// src/git/store.rs

use super::types::{GitImportStatus, GitRepoAttachment};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sqlx::SqlitePool;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use tracing::info;

#[derive(Clone)]
pub struct GitStore {
    pub pool: SqlitePool,
}

impl GitStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn create_attachment(&self, attachment: &GitRepoAttachment) -> Result<()> {
        let import_status = attachment.import_status.to_string();
        let last_imported_at = attachment.last_imported_at.map(|dt| dt.timestamp());
        let last_sync_at = attachment.last_sync_at.map(|dt| dt.timestamp());

        sqlx::query!(
            r#"
            INSERT INTO git_repo_attachments
                (id, project_id, repo_url, local_path, import_status, last_imported_at, last_sync_at)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
            attachment.id,
            attachment.project_id,
            attachment.repo_url,
            attachment.local_path,
            import_status,
            last_imported_at,
            last_sync_at,
        )
        .execute(&self.pool)
        .await
        .context("Failed to create git repo attachment")?;

        Ok(())
    }

    pub async fn get_attachments_for_project(
        &self,
        project_id: &str,
    ) -> Result<Vec<GitRepoAttachment>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, project_id, repo_url, local_path, import_status, 
                   last_imported_at, last_sync_at
            FROM git_repo_attachments
            WHERE project_id = ?
            "#,
            project_id
        )
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch git repo attachments")?;

        Ok(rows
            .into_iter()
            .map(|r| {
                let import_status = r
                    .import_status
                    .as_deref()
                    .and_then(|s| s.parse::<GitImportStatus>().ok())
                    .unwrap_or(GitImportStatus::Pending);

                let last_imported_at = r
                    .last_imported_at
                    .and_then(|ts| DateTime::from_timestamp(ts, 0))
                    .map(|dt| dt.with_timezone(&Utc));

                let last_sync_at = r
                    .last_sync_at
                    .and_then(|ts| DateTime::from_timestamp(ts, 0))
                    .map(|dt| dt.with_timezone(&Utc));

                GitRepoAttachment {
                    id: r.id.unwrap_or_default(),
                    project_id: r.project_id,
                    repo_url: r.repo_url.unwrap_or_default(),
                    local_path: r.local_path,
                    import_status,
                    last_imported_at,
                    last_sync_at,
                }
            })
            .collect())
    }

    pub async fn get_attachment(&self, attachment_id: &str) -> Result<Option<GitRepoAttachment>> {
        let r = sqlx::query!(
            r#"
            SELECT id, project_id, repo_url, local_path, import_status, 
                   last_imported_at, last_sync_at
            FROM git_repo_attachments
            WHERE id = ?
            "#,
            attachment_id
        )
        .fetch_optional(&self.pool)
        .await
        .context("Failed to fetch git repo attachment by id")?;

        Ok(r.map(|r| {
            let import_status = r
                .import_status
                .as_deref()
                .and_then(|s| s.parse::<GitImportStatus>().ok())
                .unwrap_or(GitImportStatus::Pending);

            let last_imported_at = r
                .last_imported_at
                .and_then(|ts| DateTime::from_timestamp(ts, 0))
                .map(|dt| dt.with_timezone(&Utc));

            let last_sync_at = r
                .last_sync_at
                .and_then(|ts| DateTime::from_timestamp(ts, 0))
                .map(|dt| dt.with_timezone(&Utc));

            GitRepoAttachment {
                id: r.id.unwrap_or_default(),
                project_id: r.project_id,
                repo_url: r.repo_url.unwrap_or_default(),
                local_path: r.local_path,
                import_status,
                last_imported_at,
                last_sync_at,
            }
        }))
    }

    pub async fn list_project_attachments(
        &self,
        project_id: &str,
    ) -> Result<Vec<GitRepoAttachment>> {
        self.get_attachments_for_project(project_id).await
    }

    pub async fn update_import_status(
        &self,
        attachment_id: &str,
        status: GitImportStatus,
    ) -> Result<()> {
        let status_str = status.to_string();
        sqlx::query!(
            r#"
            UPDATE git_repo_attachments
            SET import_status = ?
            WHERE id = ?
            "#,
            status_str,
            attachment_id
        )
        .execute(&self.pool)
        .await
        .context("Failed to update import status")?;

        Ok(())
    }

    pub async fn update_last_sync(&self, attachment_id: &str, dt: DateTime<Utc>) -> Result<()> {
        let timestamp = dt.timestamp();
        sqlx::query!(
            r#"
            UPDATE git_repo_attachments
            SET last_sync_at = ?
            WHERE id = ?
            "#,
            timestamp,
            attachment_id
        )
        .execute(&self.pool)
        .await
        .context("Failed to update last sync time")?;

        Ok(())
    }

    pub async fn update_last_imported(&self, attachment_id: &str, dt: DateTime<Utc>) -> Result<()> {
        let timestamp = dt.timestamp();
        sqlx::query!(
            r#"
            UPDATE git_repo_attachments
            SET last_imported_at = ?
            WHERE id = ?
            "#,
            timestamp,
            attachment_id
        )
        .execute(&self.pool)
        .await
        .context("Failed to update last imported time")?;

        Ok(())
    }

    pub async fn delete_attachment(&self, attachment_id: &str) -> Result<bool> {
        let result = sqlx::query!(
            r#"
            DELETE FROM git_repo_attachments
            WHERE id = ?
            "#,
            attachment_id
        )
        .execute(&self.pool)
        .await
        .context("Failed to delete git repo attachment")?;

        Ok(result.rows_affected() > 0)
    }

    pub async fn insert_repository_file(
        &self,
        attachment_id: &str,
        file_path: &str,
        content_hash: &str,
        language: Option<&str>,
        line_count: i64,
    ) -> Result<i64> {
        let result = sqlx::query!(
            r#"
            INSERT INTO repository_files
                (attachment_id, file_path, content_hash, language, line_count, last_indexed)
            VALUES (?, ?, ?, ?, ?, datetime('now'))
            "#,
            attachment_id,
            file_path,
            content_hash,
            language,
            line_count,
        )
        .execute(&self.pool)
        .await
        .context("Failed to insert repository file")?;

        Ok(result.last_insert_rowid())
    }

    /// Insert a file record into repository_files table (reads file from disk)
    pub async fn insert_file_record(
        &self,
        file_path: &Path,
        attachment_id: &str,
        git_dir: &Path,
    ) -> Result<i64> {
        let content = tokio::fs::read(file_path)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to read file {}: {}", file_path.display(), e))?;

        let mut hasher = DefaultHasher::new();
        content.hash(&mut hasher);
        let content_hash = format!("{:x}", hasher.finish());

        let content_str = String::from_utf8_lossy(&content);
        let line_count = content_str.lines().count() as i64;

        // Detect language based on file extension
        let language = if is_rust_file(file_path) {
            Some("rust".to_string())
        } else if is_typescript_file(file_path) {
            Some("typescript".to_string())
        } else if is_javascript_file(file_path) {
            Some("javascript".to_string())
        } else {
            None
        };

        let repo_path = git_dir.join(attachment_id);
        let relative_path = file_path
            .strip_prefix(&repo_path)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| file_path.to_string_lossy().to_string());

        self.insert_repository_file(
            attachment_id,
            &relative_path,
            &content_hash,
            language.as_deref(),
            line_count,
        )
        .await
    }

    pub async fn get_repository_files(&self, attachment_id: &str) -> Result<Vec<RepositoryFile>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, attachment_id, file_path, content_hash, language, 
                   last_indexed, line_count, function_count
            FROM repository_files
            WHERE attachment_id = ?
            ORDER BY file_path
            "#,
            attachment_id
        )
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch repository files")?;

        Ok(rows
            .into_iter()
            .map(|r| RepositoryFile {
                id: r.id.unwrap_or(0),
                attachment_id: r.attachment_id.unwrap_or_default(),
                file_path: r.file_path,
                content_hash: r.content_hash,
                language: r.language,
                last_indexed: r.last_indexed.map(|dt| dt.to_string()).unwrap_or_default(),
                line_count: r.line_count,
                function_count: r.function_count,
            })
            .collect())
    }

    pub async fn update_file_analysis(
        &self,
        file_id: i64,
        function_count: Option<i64>,
    ) -> Result<()> {
        sqlx::query!(
            r#"
            UPDATE repository_files
            SET function_count = ?
            WHERE id = ?
            "#,
            function_count,
            file_id
        )
        .execute(&self.pool)
        .await
        .context("Failed to update file analysis")?;

        Ok(())
    }

    pub async fn delete_repository_files(&self, attachment_id: &str) -> Result<()> {
        sqlx::query!(
            r#"
            DELETE FROM repository_files
            WHERE attachment_id = ?
            "#,
            attachment_id
        )
        .execute(&self.pool)
        .await
        .context("Failed to delete repository files")?;

        Ok(())
    }

    // ========== PHASE 3: LOCAL DIRECTORY SUPPORT ==========

    /// Attach a local directory to a project
    pub async fn attach_local_directory(
        &self,
        project_id: &str,
        directory_path: &str,
    ) -> Result<()> {
        info!(
            "Attaching local directory: {} to project {}",
            directory_path, project_id
        );

        // Validate path
        let path = Path::new(directory_path);
        if !path.exists() {
            anyhow::bail!("Directory does not exist: {}", directory_path);
        }
        if !path.is_dir() {
            anyhow::bail!("Path is not a directory: {}", directory_path);
        }

        let absolute_path = path.canonicalize().context("Failed to get absolute path")?;
        let path_str = absolute_path
            .to_str()
            .context("Path contains invalid UTF-8")?;

        // Insert or update attachment
        let attachment_id = uuid::Uuid::new_v4().to_string();
        sqlx::query!(
            r#"
            INSERT INTO git_repo_attachments 
                (id, project_id, repo_url, local_path, attachment_type, import_status, local_path_override)
            VALUES (?, ?, '', ?, 'local_directory', 'complete', ?)
            ON CONFLICT(project_id, repo_url) DO UPDATE SET
                attachment_type = 'local_directory',
                local_path = excluded.local_path,
                local_path_override = excluded.local_path_override,
                import_status = 'complete',
                last_sync_at = (strftime('%s', 'now'))
            "#,
            attachment_id,
            project_id,
            path_str,
            path_str
        )
        .execute(&self.pool)
        .await
        .context("Failed to attach local directory")?;

        info!("Local directory attached successfully");
        Ok(())
    }

    /// Get the base path for a project (handles both git and local directories)
    pub async fn get_project_base_path(&self, project_id: &str) -> Result<PathBuf> {
        let row = sqlx::query!(
            r#"
            SELECT attachment_type, local_path, local_path_override
            FROM git_repo_attachments
            WHERE project_id = ?
            LIMIT 1
            "#,
            project_id
        )
        .fetch_one(&self.pool)
        .await
        .context("Project has no attachment")?;

        let path = if row.attachment_type.as_deref() == Some("local_directory") {
            // Use override path for local directories
            row.local_path_override.unwrap_or(row.local_path)
        } else {
            // Use regular path for git repos
            row.local_path
        };

        Ok(PathBuf::from(path))
    }
}

#[derive(Debug, Clone)]
pub struct RepositoryFile {
    pub id: i64,
    pub attachment_id: String,
    pub file_path: String,
    pub content_hash: String,
    pub language: Option<String>,
    pub last_indexed: String,
    pub line_count: Option<i64>,
    pub function_count: Option<i64>,
}

// Helper functions for file type detection

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
