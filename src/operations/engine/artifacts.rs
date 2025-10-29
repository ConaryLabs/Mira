// src/operations/engine/artifacts.rs
// Artifact creation and management

use crate::operations::{Artifact, engine::events::OperationEngineEvent};

use anyhow::{Context, Result};
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::info;
use sha2::{Sha256, Digest};

#[derive(Clone)]
pub struct ArtifactManager {
    db: Arc<SqlitePool>,
}

impl ArtifactManager {
    pub fn new(db: Arc<SqlitePool>) -> Self {
        Self { db }
    }

    /// Create artifact from tool call arguments
    pub async fn create_artifact(
        &self,
        operation_id: &str,
        args: serde_json::Value,
        event_tx: &mpsc::Sender<OperationEngineEvent>,
    ) -> Result<()> {
        let path = args.get("path")
            .and_then(|v| v.as_str())
            .context("Missing 'path' in create_artifact")?
            .to_string();

        let content = args.get("content")
            .and_then(|v| v.as_str())
            .context("Missing 'content' in create_artifact")?
            .to_string();

        let language = args.get("language")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let explanation = args.get("explanation")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        info!("[ENGINE] Creating artifact: {} ({} bytes)", path, content.len());

        // Generate content hash
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        let hash = format!("{:x}", hasher.finalize());

        // Check if previous artifact exists
        let previous = sqlx::query!(
            "SELECT content FROM artifacts WHERE operation_id = ? AND file_path = ? ORDER BY created_at DESC LIMIT 1",
            operation_id,
            path
        )
        .fetch_optional(&*self.db)
        .await?;

        let diff = if let Some(prev) = previous {
            Some(Self::compute_diff(&prev.content, &content))
        } else {
            None
        };

        let artifact = Artifact::new(
            operation_id.to_string(),
            "code".to_string(),
            Some(path.clone()),
            content.clone(),
            hash,
            language,
            diff,
        );

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

        let _ = event_tx.send(OperationEngineEvent::ArtifactPreview {
            operation_id: operation_id.to_string(),
            artifact_id: artifact.id.clone(),
            path: path.to_string(),
            preview,
        }).await;

        let _ = event_tx.send(OperationEngineEvent::ArtifactCompleted {
            operation_id: operation_id.to_string(),
            artifact,
        }).await;

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

        let artifacts = rows.into_iter().map(|row| {
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
        }).collect();

        Ok(artifacts)
    }

    /// Compute a simple unified diff between old and new content
    fn compute_diff(old_content: &str, new_content: &str) -> String {
        let old_lines: Vec<&str> = old_content.lines().collect();
        let new_lines: Vec<&str> = new_content.lines().collect();
        
        let mut diff = String::new();
        diff.push_str(&format!("--- old\n+++ new\n"));
        
        let max_lines = old_lines.len().max(new_lines.len());
        let mut changes = Vec::new();
        
        for i in 0..max_lines {
            let old_line = old_lines.get(i).copied();
            let new_line = new_lines.get(i).copied();
            
            match (old_line, new_line) {
                (Some(old), Some(new)) if old != new => {
                    changes.push(format!("-{}", old));
                    changes.push(format!("+{}", new));
                }
                (Some(old), None) => {
                    changes.push(format!("-{}", old));
                }
                (None, Some(new)) => {
                    changes.push(format!("+{}", new));
                }
                _ => {} // Lines are the same or both None
            }
        }
        
        if !changes.is_empty() {
            diff.push_str(&format!("@@ -{},{} +{},{} @@\n", 
                1, old_lines.len(), 
                1, new_lines.len()
            ));
            for change in changes {
                diff.push_str(&change);
                diff.push('\n');
            }
        }
        
        diff
    }
}
