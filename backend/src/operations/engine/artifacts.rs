// src/operations/engine/artifacts.rs
// Artifact creation and management

use crate::operations::{Artifact, engine::events::OperationEngineEvent};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use similar::{ChangeTag, TextDiff};
use sqlx::SqlitePool;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, info};

#[derive(Clone)]
pub struct ArtifactManager {
    db: Arc<SqlitePool>,
}

impl ArtifactManager {
    pub fn new(db: Arc<SqlitePool>) -> Self {
        Self { db }
    }

    /// Create artifact from tool call arguments
    ///
    /// If `project_root` is provided, the function will check if the file exists
    /// on disk and compute a diff against the current file content (not previous
    /// artifacts in the same operation).
    pub async fn create_artifact(
        &self,
        operation_id: &str,
        args: serde_json::Value,
        event_tx: &mpsc::Sender<OperationEngineEvent>,
        project_root: Option<&Path>,
    ) -> Result<()> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .context("Missing 'path' in create_artifact")?
            .to_string();

        let content = args
            .get("content")
            .and_then(|v| v.as_str())
            .context("Missing 'content' in create_artifact")?
            .to_string();

        let language = args
            .get("language")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let explanation = args
            .get("explanation")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        info!(
            "[ENGINE] Creating artifact: {} ({} bytes)",
            path,
            content.len()
        );

        // Generate content hash
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        let hash = format!("{:x}", hasher.finalize());

        // Determine if file exists and compute diff
        let (diff, is_new_file) = if let Some(root) = project_root {
            let file_path = root.join(&path);
            if file_path.exists() && file_path.is_file() {
                // Read original file content from disk
                match tokio::fs::read_to_string(&file_path).await {
                    Ok(original_content) => {
                        debug!("[ENGINE] Computing diff against existing file: {}", path);
                        let diff = Self::compute_diff(&original_content, &content);
                        (Some(diff), false)
                    }
                    Err(e) => {
                        debug!("[ENGINE] Failed to read file {}: {}, treating as new", path, e);
                        (None, true)
                    }
                }
            } else {
                debug!("[ENGINE] File does not exist, marking as new: {}", path);
                (None, true)
            }
        } else {
            // Fallback: Check previous artifacts in DB (original behavior)
            let previous = sqlx::query!(
                "SELECT content FROM artifacts WHERE operation_id = ? AND file_path = ? ORDER BY created_at DESC LIMIT 1",
                operation_id,
                path
            )
            .fetch_optional(&*self.db)
            .await?;

            if let Some(prev) = previous {
                (Some(Self::compute_diff(&prev.content, &content)), false)
            } else {
                (None, true)
            }
        };

        let mut artifact = Artifact::new(
            operation_id.to_string(),
            "code".to_string(),
            Some(path.clone()),
            content.clone(),
            hash,
            language,
            diff,
        );

        // Set is_new_file flag
        artifact.is_new_file = Some(if is_new_file { 1 } else { 0 });

        sqlx::query!(
            r#"
            INSERT INTO artifacts (
                id, operation_id, kind, file_path, content, content_hash,
                language, diff_from_previous, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
            artifact.id,
            artifact.operation_id,
            artifact.kind,
            artifact.file_path,
            artifact.content,
            artifact.content_hash,
            artifact.language,
            artifact.diff,
            artifact.created_at,
        )
        .execute(&*self.db)
        .await
        .context("Failed to create artifact")?;

        let preview = if content.len() > 200 {
            format!("{}...", &content[..200])
        } else {
            content.to_string()
        };

        let _ = event_tx
            .send(OperationEngineEvent::ArtifactPreview {
                operation_id: operation_id.to_string(),
                artifact_id: artifact.id.clone(),
                path: path.to_string(),
                preview,
            })
            .await;

        let _ = event_tx
            .send(OperationEngineEvent::ArtifactCompleted {
                operation_id: operation_id.to_string(),
                artifact,
            })
            .await;

        if let Some(expl) = explanation {
            info!("[ENGINE] Artifact explanation: {}", expl);
        }

        Ok(())
    }

    /// Get all artifacts for an operation
    pub async fn get_artifacts_for_operation(&self, operation_id: &str) -> Result<Vec<Artifact>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, operation_id, kind, file_path, content, content_hash, language, diff_from_previous, created_at
            FROM artifacts
            WHERE operation_id = ?
            ORDER BY created_at
            "#,
            operation_id
        )
        .fetch_all(&*self.db)
        .await
        .context("Failed to fetch artifacts")?;

        let artifacts = rows
            .into_iter()
            .map(|row| {
                let mut artifact = Artifact::new(
                    row.operation_id,
                    row.kind,
                    row.file_path,
                    row.content,
                    row.content_hash.unwrap_or_default(),
                    row.language,
                    row.diff_from_previous,
                );
                // Override the auto-generated id and created_at with values from DB
                artifact.id = row.id.unwrap_or_default();
                artifact.created_at = row.created_at;
                artifact
            })
            .collect();

        Ok(artifacts)
    }

    /// Compute a unified diff between old and new content using LCS algorithm
    fn compute_diff(old_content: &str, new_content: &str) -> String {
        let diff = TextDiff::from_lines(old_content, new_content);
        let mut output = String::new();

        // Add file headers
        output.push_str("--- a/original\n");
        output.push_str("+++ b/modified\n");

        // Generate hunks with 3 lines of context
        for (idx, group) in diff.grouped_ops(3).iter().enumerate() {
            // Calculate hunk header
            let mut old_start = 0;
            let mut old_count = 0;
            let mut new_start = 0;
            let mut new_count = 0;

            for op in group {
                match op {
                    similar::DiffOp::Equal { old_index, new_index, len } => {
                        if old_start == 0 {
                            old_start = old_index + 1;
                            new_start = new_index + 1;
                        }
                        old_count += len;
                        new_count += len;
                    }
                    similar::DiffOp::Delete { old_index, old_len, .. } => {
                        if old_start == 0 {
                            old_start = old_index + 1;
                            new_start = 1;
                        }
                        old_count += old_len;
                    }
                    similar::DiffOp::Insert { new_index, new_len, .. } => {
                        if old_start == 0 {
                            old_start = 1;
                            new_start = new_index + 1;
                        }
                        new_count += new_len;
                    }
                    similar::DiffOp::Replace { old_index, old_len, new_index, new_len } => {
                        if old_start == 0 {
                            old_start = old_index + 1;
                            new_start = new_index + 1;
                        }
                        old_count += old_len;
                        new_count += new_len;
                    }
                }
            }

            // Add hunk header
            if idx > 0 || !group.is_empty() {
                output.push_str(&format!(
                    "@@ -{},{} +{},{} @@\n",
                    old_start, old_count, new_start, new_count
                ));
            }

            // Add diff lines
            for op in group {
                for change in diff.iter_changes(op) {
                    let sign = match change.tag() {
                        ChangeTag::Delete => "-",
                        ChangeTag::Insert => "+",
                        ChangeTag::Equal => " ",
                    };
                    let line = change.value();
                    output.push_str(sign);
                    output.push_str(line);
                    // Ensure line ends with newline
                    if !line.ends_with('\n') {
                        output.push('\n');
                    }
                }
            }
        }

        output
    }
}
