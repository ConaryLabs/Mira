// src/git/client/code_sync.rs
// Handles code intelligence synchronization after git operations

use anyhow::Result;
use sha2::{Digest, Sha256};
use std::path::Path;
use tracing::{debug, info, warn};

use crate::api::error::IntoApiError;
use crate::git::store::GitStore;
use crate::git::types::GitRepoAttachment;
use crate::memory::features::code_intelligence::CodeIntelligenceService;

/// Manages code intelligence synchronization with git operations
#[derive(Clone)]
pub struct CodeSync {
    store: GitStore,
    code_intelligence: CodeIntelligenceService,
}

impl CodeSync {
    pub fn new(store: GitStore, code_intelligence: CodeIntelligenceService) -> Self {
        Self {
            store,
            code_intelligence,
        }
    }

    /// Re-parse changed files after git pull (Layer 3)
    pub async fn sync_after_pull(&self, attachment: &GitRepoAttachment) -> Result<()> {
        info!(
            "Re-parsing changed files after pull for attachment {}",
            attachment.id
        );

        let local_path = attachment.local_path.clone();
        let attachment_id = attachment.id.clone();
        let project_id = attachment.project_id.clone();

        // Get list of parseable files that might have changed
        let files_to_check =
            tokio::task::spawn_blocking(move || -> Result<Vec<(String, String)>> {
                let mut files = Vec::new();

                for entry in walkdir::WalkDir::new(&local_path)
                    .follow_links(false)
                    .into_iter()
                    .filter_entry(|e| !should_ignore_path(e.path()))
                {
                    let entry = match entry {
                        Ok(e) => e,
                        Err(_) => continue,
                    };

                    if !entry.file_type().is_file() {
                        continue;
                    }

                    let path = entry.path();
                    if !is_parseable_file(path) {
                        continue;
                    }

                    // Read file content
                    let content = match std::fs::read_to_string(path) {
                        Ok(c) => c,
                        Err(_) => continue,
                    };

                    // Get relative path
                    let relative_path = match path.strip_prefix(&local_path) {
                        Ok(p) => p.to_string_lossy().to_string(),
                        Err(_) => continue,
                    };

                    files.push((relative_path, content));
                }

                Ok(files)
            })
            .await
            .into_api_error("Failed to scan directory")?
            .into_api_error("Failed to list files")?;

        // Re-parse each file
        let mut parsed_count = 0;
        for (file_path, content) in files_to_check {
            // Check if file hash changed
            let mut hasher = Sha256::new();
            hasher.update(content.as_bytes());
            let current_hash = format!("{:x}", hasher.finalize());

            let last_hash = sqlx::query_scalar!(
                r#"
                SELECT content_hash FROM repository_files
                WHERE attachment_id = ? AND file_path = ?
                "#,
                attachment_id,
                file_path
            )
            .fetch_optional(&self.store.pool)
            .await?;

            // Skip if unchanged
            if last_hash.as_deref() == Some(current_hash.as_str()) {
                continue;
            }

            // File changed - re-parse
            let language = detect_language_from_path(&file_path);

            // CRITICAL FIX: Use transaction to ensure atomicity and prevent FK violations
            let mut tx = self.store.pool.begin().await?;

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
                current_hash,
                language
            )
            .fetch_one(&mut *tx)
            .await?;

            // Step 2: Delete old code elements for this file (prevents UNIQUE constraint violations)
            sqlx::query!(
                r#"
                DELETE FROM code_elements WHERE file_id = ?
                "#,
                file_id
            )
            .execute(&mut *tx)
            .await?;

            // Step 3: Commit transaction (ensures file_id exists and old elements are gone)
            tx.commit().await?;

            // Step 4: Parse AST and store new elements
            match self
                .code_intelligence
                .analyze_and_store_with_project(
                    file_id,
                    &content,
                    &file_path,
                    &language,
                    &project_id,
                )
                .await
            {
                Ok(_) => {
                    parsed_count += 1;
                }
                Err(e) => {
                    warn!("Failed to parse {} after pull: {}", file_path, e);
                }
            }
        }

        if parsed_count > 0 {
            info!("Re-parsed {} files after pull", parsed_count);
        }

        Ok(())
    }

    /// Analyze a single file and store results
    pub async fn analyze_file(
        &self,
        file_id: i64,
        file_path: &Path,
        project_id: &str,
    ) -> Result<()> {
        let content = tokio::fs::read_to_string(file_path)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to read file {}: {}", file_path.display(), e))?;

        let file_path_str = file_path.to_string_lossy();

        // Determine language from file extension
        let language = if is_rust_file(file_path) {
            "rust"
        } else if is_typescript_file(file_path) {
            "typescript"
        } else if is_javascript_file(file_path) {
            "javascript"
        } else {
            return Ok(()); // Skip unsupported file types
        };

        // Delete old elements before re-parsing to prevent FK/UNIQUE violations
        sqlx::query!(
            r#"
            DELETE FROM code_elements WHERE file_id = ?
            "#,
            file_id
        )
        .execute(&self.store.pool)
        .await?;

        // Parse and store new elements
        let result = self
            .code_intelligence
            .analyze_and_store_with_project(file_id, &content, &file_path_str, language, project_id)
            .await?;

        debug!(
            "Analyzed {} file {} (id: {}): {} elements, complexity: {}, {} quality issues",
            language,
            file_path.display(),
            file_id,
            result.elements_count,
            result.complexity_score,
            result.quality_issues_count
        );

        Ok(())
    }
}

// Helper functions

fn is_rust_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("rs"))
        .unwrap_or(false)
}

fn is_typescript_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e == "ts" || e == "tsx")
        .unwrap_or(false)
}

fn is_javascript_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e == "js" || e == "jsx" || e == "mjs")
        .unwrap_or(false)
}

pub fn is_parseable_file(path: &Path) -> bool {
    is_rust_file(path) || is_typescript_file(path) || is_javascript_file(path)
}

pub fn should_ignore_path(path: &Path) -> bool {
    path.components().any(|c| {
        let s = c.as_os_str().to_string_lossy();
        matches!(
            s.as_ref(),
            "node_modules" | ".git" | "target" | "dist" | "build" | ".next" | "vendor" | ".cargo"
        )
    })
}

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
