// src/watcher/processor.rs
// Event processing for file changes

use anyhow::Result;
use chrono::Utc;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{debug, info, warn};

use crate::memory::features::code_intelligence::CodeIntelligenceService;

use super::config::WatcherConfig;
use super::events::{ChangeType, FileChangeEvent};
use super::registry::WatchRegistry;

/// Processes file change events and updates code intelligence
pub struct EventProcessor {
    pool: SqlitePool,
    code_intelligence: Arc<CodeIntelligenceService>,
    registry: Arc<WatchRegistry>,
    config: WatcherConfig,
}

impl EventProcessor {
    pub fn new(
        pool: SqlitePool,
        code_intelligence: Arc<CodeIntelligenceService>,
        registry: Arc<WatchRegistry>,
        config: WatcherConfig,
    ) -> Self {
        Self {
            pool,
            code_intelligence,
            registry,
            config,
        }
    }

    /// Process a batch of file change events
    pub async fn process_batch(&self, events: Vec<FileChangeEvent>) -> Result<()> {
        if events.is_empty() {
            return Ok(());
        }

        info!("Processing batch of {} file events", events.len());

        // Limit batch size
        let events_to_process: Vec<_> = events
            .into_iter()
            .take(self.config.max_batch_size)
            .collect();

        let mut processed = 0;
        let mut skipped = 0;

        for event in events_to_process {
            // Check git cooldown
            if self
                .registry
                .in_git_cooldown(&event.attachment_id, self.config.git_cooldown_ms)
            {
                debug!(
                    "Skipping {} - in git cooldown",
                    event.relative_path
                );
                skipped += 1;
                continue;
            }

            // Process the event
            match self.process_event(&event).await {
                Ok(_) => {
                    processed += 1;
                }
                Err(e) => {
                    warn!("Failed to process {}: {}", event.relative_path, e);
                }
            }

            // Delay between files to prevent CPU spikes
            if self.config.process_delay_ms > 0 {
                sleep(Duration::from_millis(self.config.process_delay_ms)).await;
            }
        }

        if processed > 0 || skipped > 0 {
            info!(
                "Batch complete: {} processed, {} skipped (git cooldown)",
                processed, skipped
            );
        }

        Ok(())
    }

    /// Process a single file change event
    async fn process_event(&self, event: &FileChangeEvent) -> Result<()> {
        debug!(
            "Processing {:?} for {}",
            event.change_type, event.relative_path
        );

        match event.change_type {
            ChangeType::Created | ChangeType::Modified => {
                self.process_create_or_modify(event).await
            }
            ChangeType::Deleted => self.process_delete(event).await,
        }
    }

    /// Process a file creation or modification
    async fn process_create_or_modify(&self, event: &FileChangeEvent) -> Result<()> {
        // Read file content
        let content = match tokio::fs::read_to_string(&event.path).await {
            Ok(c) => c,
            Err(e) => {
                warn!("Failed to read {:?}: {}", event.path, e);
                return Ok(());
            }
        };

        // Calculate hash
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        let current_hash = format!("{:x}", hasher.finalize());

        // Get attachment_id for this path
        let attachment_id = self.get_attachment_id(&event.project_id).await?;

        // Check if hash changed
        let (file_id, old_hash) = self
            .get_file_record(&attachment_id, &event.relative_path)
            .await?;

        if let Some(ref existing_hash) = old_hash {
            if existing_hash == &current_hash {
                debug!("File unchanged (same hash): {}", event.relative_path);
                return Ok(());
            }
        }

        // Log the change to local_changes
        let change_type = if old_hash.is_some() {
            "modified"
        } else {
            "created"
        };
        self.log_file_change(
            &event.project_id,
            &event.relative_path,
            change_type,
            old_hash.as_deref(),
            Some(&current_hash),
        )
        .await?;

        // If we have an existing file_id, invalidate old embeddings
        if let Some(fid) = file_id {
            if let Err(e) = self.code_intelligence.invalidate_file(fid).await {
                warn!("Failed to invalidate embeddings for file {}: {}", fid, e);
            }
        }

        // Upsert file record and process
        let language = detect_language(&event.relative_path);
        let new_file_id = self
            .upsert_file_record(&attachment_id, &event.relative_path, &current_hash, &language)
            .await?;

        // Delete old code elements
        sqlx::query!("DELETE FROM code_elements WHERE file_id = ?", new_file_id)
            .execute(&self.pool)
            .await?;

        // Parse and store new elements
        self.code_intelligence
            .analyze_and_store_with_project(
                new_file_id,
                &content,
                &event.relative_path,
                &language,
                &event.project_id,
            )
            .await?;

        // Embed the code elements
        match self
            .code_intelligence
            .embed_code_elements(new_file_id, &event.project_id)
            .await
        {
            Ok(count) => {
                if count > 0 {
                    info!(
                        "Embedded {} code elements from {} (watcher)",
                        count, event.relative_path
                    );
                }
            }
            Err(e) => {
                warn!(
                    "Failed to embed code elements for {}: {}",
                    event.relative_path, e
                );
            }
        }

        Ok(())
    }

    /// Process a file deletion
    async fn process_delete(&self, event: &FileChangeEvent) -> Result<()> {
        let attachment_id = self.get_attachment_id(&event.project_id).await?;

        // Get file record
        let (file_id, old_hash) = self
            .get_file_record(&attachment_id, &event.relative_path)
            .await?;

        if let Some(fid) = file_id {
            info!(
                "Processing deletion: {} (file_id: {})",
                event.relative_path, fid
            );

            // Invalidate embeddings
            if let Err(e) = self.code_intelligence.invalidate_file(fid).await {
                warn!("Failed to invalidate embeddings for deleted file {}: {}", fid, e);
            }

            // Delete code elements
            sqlx::query!("DELETE FROM code_elements WHERE file_id = ?", fid)
                .execute(&self.pool)
                .await?;

            // Delete file record
            sqlx::query!("DELETE FROM repository_files WHERE id = ?", fid)
                .execute(&self.pool)
                .await?;

            // Log the deletion
            self.log_file_change(
                &event.project_id,
                &event.relative_path,
                "deleted",
                old_hash.as_deref(),
                None,
            )
            .await?;
        }

        Ok(())
    }

    /// Get attachment_id for a project
    async fn get_attachment_id(&self, project_id: &str) -> Result<String> {
        let result = sqlx::query_scalar!(
            r#"
            SELECT id FROM git_repo_attachments
            WHERE project_id = ? AND import_status = 'complete'
            LIMIT 1
            "#,
            project_id
        )
        .fetch_optional(&self.pool)
        .await?;

        result
            .flatten()
            .ok_or_else(|| anyhow::anyhow!("No attachment found for project {}", project_id))
    }

    /// Get file record from repository_files
    async fn get_file_record(
        &self,
        attachment_id: &str,
        file_path: &str,
    ) -> Result<(Option<i64>, Option<String>)> {
        let result: Option<(Option<i64>, Option<String>)> = sqlx::query_as(
            r#"
            SELECT id, content_hash FROM repository_files
            WHERE attachment_id = ? AND file_path = ?
            "#,
        )
        .bind(attachment_id)
        .bind(file_path)
        .fetch_optional(&self.pool)
        .await?;

        match result {
            Some((id, hash)) => Ok((id, hash)),
            None => Ok((None, None)),
        }
    }

    /// Upsert a file record and return the file_id
    async fn upsert_file_record(
        &self,
        attachment_id: &str,
        file_path: &str,
        content_hash: &str,
        language: &str,
    ) -> Result<i64> {
        let file_id = sqlx::query_scalar!(
            r#"
            INSERT INTO repository_files (attachment_id, file_path, content_hash, language, last_indexed)
            VALUES (?, ?, ?, ?, strftime('%s','now'))
            ON CONFLICT(attachment_id, file_path) DO UPDATE SET
                content_hash = excluded.content_hash,
                language = excluded.language,
                last_indexed = strftime('%s','now')
            RETURNING id
            "#,
            attachment_id,
            file_path,
            content_hash,
            language
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(file_id)
    }

    /// Log a file change to local_changes table
    async fn log_file_change(
        &self,
        project_id: &str,
        file_path: &str,
        change_type: &str,
        old_hash: Option<&str>,
        new_hash: Option<&str>,
    ) -> Result<()> {
        let created_at = Utc::now().timestamp();

        sqlx::query!(
            r#"
            INSERT INTO local_changes (project_id, file_path, change_type, old_hash, new_hash, created_at)
            VALUES (?, ?, ?, ?, ?, ?)
            "#,
            project_id,
            file_path,
            change_type,
            old_hash,
            new_hash,
            created_at
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}

/// Detect programming language from file path
fn detect_language(path: &str) -> String {
    if path.ends_with(".rs") {
        "rust".to_string()
    } else if path.ends_with(".ts") || path.ends_with(".tsx") {
        "typescript".to_string()
    } else if path.ends_with(".js") || path.ends_with(".jsx") || path.ends_with(".mjs") {
        "javascript".to_string()
    } else {
        "unknown".to_string()
    }
}
