// src/git/store.rs

use anyhow::{Result, Context};
use sqlx::SqlitePool;
use super::types::{GitRepoAttachment, GitImportStatus};
use chrono::{DateTime, Utc};

#[derive(Clone)]
pub struct GitStore {
    pub pool: SqlitePool,
}

impl GitStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn create_attachment(&self, attachment: &GitRepoAttachment) -> Result<()> {
        // Store status as string and timestamps as ISO8601 strings
        let import_status = attachment.import_status.to_string();
        let last_imported_at = attachment.last_imported_at.map(|dt| dt.to_rfc3339());
        let last_sync_at = attachment.last_sync_at.map(|dt| dt.to_rfc3339());

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

    pub async fn get_attachments_for_project(&self, project_id: &str) -> Result<Vec<GitRepoAttachment>> {
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

        Ok(rows.into_iter().map(|r| {
            // Parse status from string
            let import_status = r.import_status
                .parse::<GitImportStatus>()
                .unwrap_or(GitImportStatus::Pending);
            
            // Parse timestamps - SQLite stores them as INTEGER (Unix timestamp)
            let last_imported_at = r.last_imported_at
                .and_then(|ts| DateTime::from_timestamp(ts, 0));
            
            let last_sync_at = r.last_sync_at
                .and_then(|ts| DateTime::from_timestamp(ts, 0));

            GitRepoAttachment {
                id: r.id,
                project_id: r.project_id,
                repo_url: r.repo_url,
                local_path: r.local_path,
                import_status,
                last_imported_at,
                last_sync_at,
            }
        }).collect())
    }

    /// Alias for get_attachments_for_project to match the handler's expectation
    pub async fn list_project_attachments(&self, project_id: &str) -> Result<Vec<GitRepoAttachment>> {
        self.get_attachments_for_project(project_id).await
    }

    pub async fn get_attachment_by_id(&self, attachment_id: &str) -> Result<Option<GitRepoAttachment>> {
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
            // Parse status from string
            let import_status = r.import_status
                .parse::<GitImportStatus>()
                .unwrap_or(GitImportStatus::Pending);
            
            // Parse timestamps - SQLite stores them as INTEGER (Unix timestamp)
            let last_imported_at = r.last_imported_at
                .and_then(|ts| DateTime::from_timestamp(ts, 0));
            
            let last_sync_at = r.last_sync_at
                .and_then(|ts| DateTime::from_timestamp(ts, 0));

            GitRepoAttachment {
                id: r.id,
                project_id: r.project_id,
                repo_url: r.repo_url,
                local_path: r.local_path,
                import_status,
                last_imported_at,
                last_sync_at,
            }
        }))
    }

    pub async fn update_import_status(&self, attachment_id: &str, status: GitImportStatus) -> Result<()> {
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
}
