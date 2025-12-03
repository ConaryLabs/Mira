// backend/src/checkpoint/mod.rs
// Checkpoint/Rewind System - captures file state before edits for easy rollback

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::{debug, info, warn};

/// A checkpoint represents a snapshot point in time
#[derive(Debug, Clone)]
pub struct Checkpoint {
    pub id: String,
    pub session_id: String,
    pub operation_id: Option<String>,
    pub tool_name: Option<String>,
    pub description: Option<String>,
    pub created_at: i64,
    pub file_count: i32,
}

/// A file snapshot within a checkpoint
#[derive(Debug, Clone)]
pub struct CheckpointFile {
    pub id: String,
    pub checkpoint_id: String,
    pub file_path: String,
    pub content: Option<Vec<u8>>,
    pub existed: bool,
    pub file_hash: Option<String>,
}

/// Result of a restore operation
#[derive(Debug, Clone)]
pub struct RestoreResult {
    pub checkpoint_id: String,
    pub files_restored: Vec<String>,
    pub files_created: Vec<String>,
    pub files_deleted: Vec<String>,
    pub errors: Vec<String>,
}

/// Manages checkpoints for file state snapshots
pub struct CheckpointManager {
    db: SqlitePool,
    project_dir: PathBuf,
}

impl CheckpointManager {
    pub fn new(db: SqlitePool, project_dir: PathBuf) -> Self {
        Self { db, project_dir }
    }

    /// Create a checkpoint before modifying files
    pub async fn create_checkpoint(
        &self,
        session_id: &str,
        operation_id: Option<&str>,
        tool_name: Option<&str>,
        file_paths: &[&str],
        description: Option<&str>,
    ) -> Result<String> {
        let checkpoint_id = uuid::Uuid::new_v4().to_string();

        // Insert checkpoint record
        sqlx::query(
            r#"
            INSERT INTO checkpoints (id, session_id, operation_id, tool_name, description)
            VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind(&checkpoint_id)
        .bind(session_id)
        .bind(operation_id)
        .bind(tool_name)
        .bind(description)
        .execute(&self.db)
        .await
        .context("Failed to create checkpoint")?;

        // Snapshot each file
        for file_path in file_paths {
            if let Err(e) = self.snapshot_file(&checkpoint_id, file_path).await {
                warn!("Failed to snapshot file {}: {}", file_path, e);
            }
        }

        info!(
            "[CHECKPOINT] Created {} with {} files",
            &checkpoint_id[..8],
            file_paths.len()
        );

        Ok(checkpoint_id)
    }

    /// Snapshot a single file's content
    async fn snapshot_file(&self, checkpoint_id: &str, file_path: &str) -> Result<()> {
        let file_id = uuid::Uuid::new_v4().to_string();
        let full_path = self.resolve_path(file_path);

        let (content, existed, file_hash) = if full_path.exists() {
            let content = fs::read(&full_path)
                .await
                .context("Failed to read file for snapshot")?;

            // Calculate hash for deduplication
            let mut hasher = Sha256::new();
            hasher.update(&content);
            let hash = format!("{:x}", hasher.finalize());

            (Some(content), true, Some(hash))
        } else {
            // File doesn't exist yet - record that
            (None, false, None)
        };

        sqlx::query(
            r#"
            INSERT INTO checkpoint_files (id, checkpoint_id, file_path, content, existed, file_hash)
            VALUES (?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&file_id)
        .bind(checkpoint_id)
        .bind(file_path)
        .bind(&content)
        .bind(existed)
        .bind(&file_hash)
        .execute(&self.db)
        .await
        .context("Failed to save file snapshot")?;

        debug!(
            "[CHECKPOINT] Snapshotted {} (existed: {}, hash: {})",
            file_path,
            existed,
            file_hash.as_deref().unwrap_or("none")
        );

        Ok(())
    }

    /// List checkpoints for a session (most recent first)
    pub async fn list_checkpoints(&self, session_id: &str, limit: i32) -> Result<Vec<Checkpoint>> {
        let rows = sqlx::query_as::<_, (String, String, Option<String>, Option<String>, Option<String>, i64)>(
            r#"
            SELECT
                c.id, c.session_id, c.operation_id, c.tool_name, c.description, c.created_at
            FROM checkpoints c
            WHERE c.session_id = ?
            ORDER BY c.created_at DESC
            LIMIT ?
            "#,
        )
        .bind(session_id)
        .bind(limit)
        .fetch_all(&self.db)
        .await
        .context("Failed to list checkpoints")?;

        let mut checkpoints = Vec::new();
        for (id, session_id, operation_id, tool_name, description, created_at) in rows {
            // Get file count
            let file_count: (i32,) = sqlx::query_as(
                "SELECT COUNT(*) FROM checkpoint_files WHERE checkpoint_id = ?",
            )
            .bind(&id)
            .fetch_one(&self.db)
            .await
            .unwrap_or((0,));

            checkpoints.push(Checkpoint {
                id,
                session_id,
                operation_id,
                tool_name,
                description,
                created_at,
                file_count: file_count.0,
            });
        }

        Ok(checkpoints)
    }

    /// Get files in a checkpoint
    pub async fn get_checkpoint_files(&self, checkpoint_id: &str) -> Result<Vec<CheckpointFile>> {
        let rows = sqlx::query_as::<_, (String, String, String, Option<Vec<u8>>, bool, Option<String>)>(
            r#"
            SELECT id, checkpoint_id, file_path, content, existed, file_hash
            FROM checkpoint_files
            WHERE checkpoint_id = ?
            "#,
        )
        .bind(checkpoint_id)
        .fetch_all(&self.db)
        .await
        .context("Failed to get checkpoint files")?;

        Ok(rows
            .into_iter()
            .map(|(id, checkpoint_id, file_path, content, existed, file_hash)| CheckpointFile {
                id,
                checkpoint_id,
                file_path,
                content,
                existed,
                file_hash,
            })
            .collect())
    }

    /// Restore files to their state at a checkpoint
    pub async fn restore_checkpoint(&self, checkpoint_id: &str) -> Result<RestoreResult> {
        let files = self.get_checkpoint_files(checkpoint_id).await?;

        let mut result = RestoreResult {
            checkpoint_id: checkpoint_id.to_string(),
            files_restored: Vec::new(),
            files_created: Vec::new(),
            files_deleted: Vec::new(),
            errors: Vec::new(),
        };

        for file in files {
            let full_path = self.resolve_path(&file.file_path);

            if file.existed {
                // File existed at checkpoint - restore its content
                if let Some(content) = &file.content {
                    match self.restore_file(&full_path, content).await {
                        Ok(_) => {
                            if full_path.exists() {
                                result.files_restored.push(file.file_path.clone());
                            } else {
                                result.files_created.push(file.file_path.clone());
                            }
                        }
                        Err(e) => {
                            result.errors.push(format!("{}: {}", file.file_path, e));
                        }
                    }
                }
            } else {
                // File didn't exist at checkpoint - delete it if it exists now
                if full_path.exists() {
                    match fs::remove_file(&full_path).await {
                        Ok(_) => {
                            result.files_deleted.push(file.file_path.clone());
                        }
                        Err(e) => {
                            result.errors.push(format!("{}: {}", file.file_path, e));
                        }
                    }
                }
            }
        }

        info!(
            "[CHECKPOINT] Restored {}: {} restored, {} created, {} deleted, {} errors",
            &checkpoint_id[..8],
            result.files_restored.len(),
            result.files_created.len(),
            result.files_deleted.len(),
            result.errors.len()
        );

        Ok(result)
    }

    /// Restore a single file
    async fn restore_file(&self, path: &Path, content: &[u8]) -> Result<()> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .await
                .context("Failed to create parent directory")?;
        }

        fs::write(path, content)
            .await
            .context("Failed to write file")?;

        Ok(())
    }

    /// Delete old checkpoints (keep last N per session)
    pub async fn cleanup_old_checkpoints(&self, session_id: &str, keep_count: i32) -> Result<i32> {
        // Get IDs of checkpoints to delete
        let to_delete: Vec<(String,)> = sqlx::query_as(
            r#"
            SELECT id FROM checkpoints
            WHERE session_id = ?
            ORDER BY created_at DESC
            LIMIT -1 OFFSET ?
            "#,
        )
        .bind(session_id)
        .bind(keep_count)
        .fetch_all(&self.db)
        .await?;

        let count = to_delete.len() as i32;

        for (id,) in to_delete {
            // Files are deleted by CASCADE
            sqlx::query("DELETE FROM checkpoints WHERE id = ?")
                .bind(&id)
                .execute(&self.db)
                .await?;
        }

        if count > 0 {
            info!(
                "[CHECKPOINT] Cleaned up {} old checkpoints for session {}",
                count,
                &session_id[..8]
            );
        }

        Ok(count)
    }

    /// Delete all checkpoints for a session
    pub async fn clear_session_checkpoints(&self, session_id: &str) -> Result<i32> {
        let result = sqlx::query("DELETE FROM checkpoints WHERE session_id = ?")
            .bind(session_id)
            .execute(&self.db)
            .await?;

        Ok(result.rows_affected() as i32)
    }

    /// Get a specific checkpoint
    pub async fn get_checkpoint(&self, checkpoint_id: &str) -> Result<Option<Checkpoint>> {
        let row = sqlx::query_as::<_, (String, String, Option<String>, Option<String>, Option<String>, i64)>(
            r#"
            SELECT id, session_id, operation_id, tool_name, description, created_at
            FROM checkpoints
            WHERE id = ?
            "#,
        )
        .bind(checkpoint_id)
        .fetch_optional(&self.db)
        .await
        .context("Failed to get checkpoint")?;

        match row {
            Some((id, session_id, operation_id, tool_name, description, created_at)) => {
                let file_count: (i32,) = sqlx::query_as(
                    "SELECT COUNT(*) FROM checkpoint_files WHERE checkpoint_id = ?",
                )
                .bind(&id)
                .fetch_one(&self.db)
                .await
                .unwrap_or((0,));

                Ok(Some(Checkpoint {
                    id,
                    session_id,
                    operation_id,
                    tool_name,
                    description,
                    created_at,
                    file_count: file_count.0,
                }))
            }
            None => Ok(None),
        }
    }

    /// Resolve a file path relative to project directory
    fn resolve_path(&self, file_path: &str) -> PathBuf {
        let path = Path::new(file_path);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.project_dir.join(path)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    async fn create_test_db() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("Failed to create test database");

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY
            );
            CREATE TABLE IF NOT EXISTS checkpoints (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                operation_id TEXT,
                tool_name TEXT,
                description TEXT,
                created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
            );
            CREATE TABLE IF NOT EXISTS checkpoint_files (
                id TEXT PRIMARY KEY,
                checkpoint_id TEXT NOT NULL,
                file_path TEXT NOT NULL,
                content BLOB,
                existed INTEGER NOT NULL DEFAULT 1,
                file_hash TEXT
            );
            "#,
        )
        .execute(&pool)
        .await
        .expect("Failed to create tables");

        pool
    }

    #[tokio::test]
    async fn test_create_checkpoint() {
        let db = create_test_db().await;
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let manager = CheckpointManager::new(db, temp_dir.path().to_path_buf());

        // Create a test file
        let test_file = temp_dir.path().join("test.txt");
        fs::write(&test_file, "Hello, World!").await.unwrap();

        // Create checkpoint
        let checkpoint_id = manager
            .create_checkpoint("session-1", Some("op-1"), Some("write_file"), &["test.txt"], Some("Before edit"))
            .await
            .expect("Failed to create checkpoint");

        assert!(!checkpoint_id.is_empty());

        // Verify checkpoint exists
        let checkpoint = manager.get_checkpoint(&checkpoint_id).await.unwrap();
        assert!(checkpoint.is_some());
        let cp = checkpoint.unwrap();
        assert_eq!(cp.session_id, "session-1");
        assert_eq!(cp.file_count, 1);
    }

    #[tokio::test]
    async fn test_restore_checkpoint() {
        let db = create_test_db().await;
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let manager = CheckpointManager::new(db, temp_dir.path().to_path_buf());

        // Create a test file
        let test_file = temp_dir.path().join("test.txt");
        fs::write(&test_file, "Original content").await.unwrap();

        // Create checkpoint
        let checkpoint_id = manager
            .create_checkpoint("session-1", None, None, &["test.txt"], None)
            .await
            .unwrap();

        // Modify the file
        fs::write(&test_file, "Modified content").await.unwrap();

        // Verify file was modified
        let content = fs::read_to_string(&test_file).await.unwrap();
        assert_eq!(content, "Modified content");

        // Restore checkpoint
        let result = manager.restore_checkpoint(&checkpoint_id).await.unwrap();
        assert_eq!(result.files_restored.len(), 1);
        assert!(result.errors.is_empty());

        // Verify file was restored
        let content = fs::read_to_string(&test_file).await.unwrap();
        assert_eq!(content, "Original content");
    }

    #[tokio::test]
    async fn test_restore_nonexistent_file() {
        let db = create_test_db().await;
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let manager = CheckpointManager::new(db, temp_dir.path().to_path_buf());

        // Create checkpoint for file that doesn't exist
        let checkpoint_id = manager
            .create_checkpoint("session-1", None, None, &["new_file.txt"], None)
            .await
            .unwrap();

        // Create the file
        let test_file = temp_dir.path().join("new_file.txt");
        fs::write(&test_file, "New content").await.unwrap();

        // Restore checkpoint - file should be deleted
        let result = manager.restore_checkpoint(&checkpoint_id).await.unwrap();
        assert_eq!(result.files_deleted.len(), 1);

        // Verify file was deleted
        assert!(!test_file.exists());
    }

    #[tokio::test]
    async fn test_list_checkpoints() {
        let db = create_test_db().await;
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let manager = CheckpointManager::new(db, temp_dir.path().to_path_buf());

        // Create multiple checkpoints
        manager
            .create_checkpoint("session-1", None, None, &[], Some("First"))
            .await
            .unwrap();
        manager
            .create_checkpoint("session-1", None, None, &[], Some("Second"))
            .await
            .unwrap();
        manager
            .create_checkpoint("session-2", None, None, &[], Some("Other session"))
            .await
            .unwrap();

        // List checkpoints for session-1
        let checkpoints = manager.list_checkpoints("session-1", 10).await.unwrap();
        assert_eq!(checkpoints.len(), 2);
    }

    #[tokio::test]
    async fn test_cleanup_old_checkpoints() {
        let db = create_test_db().await;
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let manager = CheckpointManager::new(db, temp_dir.path().to_path_buf());

        // Create 5 checkpoints
        for i in 0..5 {
            manager
                .create_checkpoint("session-1", None, None, &[], Some(&format!("Checkpoint {}", i)))
                .await
                .unwrap();
        }

        // Keep only 2
        let deleted = manager.cleanup_old_checkpoints("session-1", 2).await.unwrap();
        assert_eq!(deleted, 3);

        // Verify only 2 remain
        let checkpoints = manager.list_checkpoints("session-1", 10).await.unwrap();
        assert_eq!(checkpoints.len(), 2);
    }
}
