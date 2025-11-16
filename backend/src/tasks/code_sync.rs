// src/tasks/code_sync.rs
// Background task to keep code intelligence up-to-date

use anyhow::Result;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use std::path::Path;
use std::sync::Arc;
use tracing::{info, warn};
use walkdir::WalkDir;

use crate::memory::features::code_intelligence::CodeIntelligenceService;

pub struct CodeSyncTask {
    pool: SqlitePool,
    code_intelligence: Arc<CodeIntelligenceService>,
}

impl CodeSyncTask {
    pub fn new(pool: SqlitePool, code_intelligence: Arc<CodeIntelligenceService>) -> Self {
        Self {
            pool,
            code_intelligence,
        }
    }

    /// Run sync for all projects with attachments
    pub async fn run(&self) -> Result<()> {
        info!("Starting code sync task");

        // Get all projects with attachments (git repos or local directories)
        let attachments = sqlx::query!(
            r#"
            SELECT id, project_id, local_path, attachment_type
            FROM git_repo_attachments
            WHERE import_status = 'complete'
            "#
        )
        .fetch_all(&self.pool)
        .await?;

        let mut total_synced = 0;

        for attachment in attachments {
            let local_path = if attachment.attachment_type.as_deref() == Some("local_directory") {
                attachment.local_path
            } else {
                attachment.local_path
            };

            match self
                .sync_attachment(&attachment.id, &attachment.project_id, &local_path)
                .await
            {
                Ok(count) => {
                    total_synced += count;
                    if count > 0 {
                        info!(
                            "Synced {} files for project {}",
                            count, attachment.project_id
                        );
                    }
                }
                Err(e) => {
                    warn!("Failed to sync project {}: {}", attachment.project_id, e);
                }
            }
        }

        if total_synced > 0 {
            info!(
                "Code sync complete: {} files updated across all projects",
                total_synced
            );
        }

        Ok(())
    }

    /// Sync a single attachment (git repo or local directory)
    async fn sync_attachment(
        &self,
        attachment_id: &str,
        project_id: &str,
        base_path: &str,
    ) -> Result<usize> {
        let mut synced = 0;

        // Walk all files in the directory
        for entry in WalkDir::new(base_path)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| !is_ignored(e.path()))
        {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    warn!("Failed to read directory entry: {}", e);
                    continue;
                }
            };

            if !entry.file_type().is_file() {
                continue;
            }

            let path = entry.path();
            if !should_parse(path) {
                continue;
            }

            // Read file content
            let content = match tokio::fs::read_to_string(path).await {
                Ok(c) => c,
                Err(e) => {
                    warn!("Failed to read file {:?}: {}", path, e);
                    continue;
                }
            };

            // Get relative path
            let relative_path = match path.strip_prefix(base_path) {
                Ok(p) => p.to_string_lossy().to_string(),
                Err(e) => {
                    warn!("Failed to get relative path for {:?}: {}", path, e);
                    continue;
                }
            };

            // Check if file changed since last parse
            let mut hasher = Sha256::new();
            hasher.update(content.as_bytes());
            let current_hash = format!("{:x}", hasher.finalize());

            let last_hash = sqlx::query_scalar!(
                r#"
                SELECT content_hash FROM repository_files
                WHERE attachment_id = ? AND file_path = ?
                "#,
                attachment_id,
                relative_path
            )
            .fetch_optional(&self.pool)
            .await?;

            // Skip if unchanged
            if last_hash.as_deref() == Some(current_hash.as_str()) {
                continue;
            }

            // File changed or is new - re-parse and embed
            match self
                .upsert_and_parse(
                    attachment_id,
                    &relative_path,
                    &content,
                    &current_hash,
                    project_id,
                )
                .await
            {
                Ok(_) => {
                    synced += 1;
                }
                Err(e) => {
                    warn!("Failed to parse {}: {}", relative_path, e);
                }
            }
        }

        Ok(synced)
    }

    /// Upsert repository_files record and trigger AST parsing + embedding
    async fn upsert_and_parse(
        &self,
        attachment_id: &str,
        file_path: &str,
        content: &str,
        content_hash: &str,
        project_id: &str,
    ) -> Result<()> {
        let language = detect_language_from_path(file_path);

        let mut tx = self.pool.begin().await?;

        // Step 1: Upsert file record
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
        .fetch_one(&mut *tx)
        .await?;

        // Step 2: Delete old code elements for this file
        sqlx::query!(
            r#"
            DELETE FROM code_elements WHERE file_id = ?
            "#,
            file_id
        )
        .execute(&mut *tx)
        .await?;

        // Step 3: Commit transaction
        tx.commit().await?;

        // Step 4: Invalidate old embeddings
        if let Err(e) = self.code_intelligence.invalidate_file(file_id).await {
            warn!(
                "Failed to invalidate embeddings for file {}: {}",
                file_id, e
            );
        }

        // Step 5: Parse AST and store new elements
        self.code_intelligence
            .analyze_and_store_with_project(file_id, content, file_path, &language, project_id)
            .await?;

        // Step 6: Embed the code elements
        match self
            .code_intelligence
            .embed_code_elements(file_id, project_id)
            .await
        {
            Ok(count) => {
                if count > 0 {
                    info!("Embedded {} code elements from {}", count, file_path);
                }
            }
            Err(e) => {
                warn!("Failed to embed code elements for {}: {}", file_path, e);
            }
        }

        Ok(())
    }
}

/// Check if file should be parsed for code intelligence
fn should_parse(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|ext| matches!(ext, "rs" | "ts" | "tsx" | "js" | "jsx"))
        .unwrap_or(false)
}

/// Check if path should be ignored (node_modules, .git, target, etc.)
fn is_ignored(path: &Path) -> bool {
    path.components().any(|c| {
        let s = c.as_os_str().to_string_lossy();
        matches!(
            s.as_ref(),
            "node_modules" | ".git" | "target" | "dist" | "build" | ".next" | "vendor" | ".cargo"
        )
    })
}

/// Detect programming language from file extension
fn detect_language_from_path(path: &str) -> String {
    if path.ends_with(".rs") {
        "rust".to_string()
    } else if path.ends_with(".ts") || path.ends_with(".tsx") {
        "typescript".to_string()
    } else if path.ends_with(".js") || path.ends_with(".jsx") {
        "javascript".to_string()
    } else {
        "unknown".to_string()
    }
}
