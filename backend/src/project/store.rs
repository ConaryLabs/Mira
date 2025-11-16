// src/project/store.rs

use crate::project::types::{Artifact, ArtifactType, Project};
use anyhow::Result;
use chrono::{NaiveDateTime, TimeZone, Utc};
use sqlx::{Row, SqlitePool};
use std::path::Path;
use tracing::{info, warn};
use uuid::Uuid;

pub struct ProjectStore {
    pub pool: SqlitePool,
}

impl ProjectStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    // Project operations

    pub async fn create_project(
        &self,
        name: String,
        description: Option<String>,
        tags: Option<Vec<String>>,
        owner: Option<String>,
    ) -> Result<Project> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let tags_json = tags
            .as_ref()
            .map(|t| serde_json::to_string(t).unwrap_or("[]".to_string()));

        sqlx::query(
            r#"
            INSERT INTO projects (id, name, description, tags, owner, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&id)
        .bind(&name)
        .bind(&description)
        .bind(&tags_json)
        .bind(&owner)
        .bind(now.naive_utc())
        .bind(now.naive_utc())
        .execute(&self.pool)
        .await?;

        Ok(Project {
            id,
            name,
            description,
            tags,
            owner,
            created_at: now,
            updated_at: now,
        })
    }

    pub async fn get_project(&self, id: &str) -> Result<Option<Project>> {
        let row = sqlx::query(
            r#"
            SELECT id, name, description, tags, owner, created_at, updated_at
            FROM projects
            WHERE id = ?
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(row) => Ok(Some(self.row_to_project(row)?)),
            None => Ok(None),
        }
    }

    pub async fn list_projects(&self) -> Result<Vec<Project>> {
        let rows = sqlx::query(
            r#"
            SELECT id, name, description, tags, owner, created_at, updated_at
            FROM projects
            ORDER BY updated_at DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|row| self.row_to_project(row))
            .collect()
    }

    pub async fn update_project(
        &self,
        id: &str,
        name: Option<String>,
        description: Option<String>,
        tags: Option<Vec<String>>,
    ) -> Result<Option<Project>> {
        // First check if project exists
        let existing = self.get_project(id).await?;
        if existing.is_none() {
            return Ok(None);
        }

        let mut project = existing.unwrap();

        // Update fields if provided
        if let Some(n) = name {
            project.name = n;
        }
        if description.is_some() {
            project.description = description;
        }
        if tags.is_some() {
            project.tags = tags;
        }

        project.updated_at = Utc::now();

        let tags_json = project
            .tags
            .as_ref()
            .map(|t| serde_json::to_string(t).unwrap_or("[]".to_string()));

        sqlx::query(
            r#"
            UPDATE projects
            SET name = ?, description = ?, tags = ?, updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(&project.name)
        .bind(&project.description)
        .bind(&tags_json)
        .bind(project.updated_at.naive_utc())
        .bind(id)
        .execute(&self.pool)
        .await?;

        Ok(Some(project))
    }

    pub async fn delete_project(&self, id: &str) -> Result<bool> {
        // Step 1: Get all git repo attachments WITH their type
        let repo_paths = sqlx::query(
            r#"
            SELECT local_path, attachment_type
            FROM git_repo_attachments
            WHERE project_id = ?
            "#,
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await?;

        // Step 2: ONLY delete git repository clones, NOT local directories
        let mut deleted_count = 0;
        for row in &repo_paths {
            let local_path: String = row.get("local_path");
            let attachment_type: Option<String> = row.get("attachment_type");

            // CRITICAL FIX: Skip local directories - they're user source code!
            if attachment_type.as_deref() == Some("local_directory") {
                info!(
                    "Skipping local directory (not deleting user source): {}",
                    local_path
                );
                continue;
            }

            // Only delete sandboxed git clones
            let path = Path::new(&local_path);
            if path.exists() {
                match tokio::fs::remove_dir_all(path).await {
                    Ok(_) => {
                        info!("Deleted git clone directory: {}", local_path);
                        deleted_count += 1;
                    }
                    Err(e) => {
                        // Log but don't fail - orphaned directories can be manually cleaned
                        warn!("Failed to delete git clone directory {}: {}", local_path, e);
                    }
                }
            } else {
                // Directory already gone, no problem
                info!("Git clone directory already removed: {}", local_path);
            }
        }

        // Step 3: Delete the project (CASCADE handles DB cleanup)
        let result = sqlx::query("DELETE FROM projects WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;

        let deleted = result.rows_affected() > 0;

        if deleted {
            info!(
                "Deleted project {} and cleaned up {} git clone directories",
                id, deleted_count
            );
        }

        Ok(deleted)
    }

    // Artifact operations

    pub async fn create_artifact(
        &self,
        project_id: String,
        name: String,
        artifact_type: ArtifactType,
        content: Option<String>,
    ) -> Result<Artifact> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();

        sqlx::query(
            r#"
            INSERT INTO artifacts (id, project_id, name, type, content, version, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&id)
        .bind(&project_id)
        .bind(&name)
        .bind(artifact_type.to_string())
        .bind(&content)
        .bind(1)
        .bind(now.naive_utc())
        .bind(now.naive_utc())
        .execute(&self.pool)
        .await?;

        Ok(Artifact {
            id,
            project_id,
            name,
            artifact_type,
            content,
            version: 1,
            created_at: now,
            updated_at: now,
        })
    }

    pub async fn get_artifact(&self, id: &str) -> Result<Option<Artifact>> {
        let row = sqlx::query(
            r#"
            SELECT id, project_id, name, type, content, version, created_at, updated_at
            FROM artifacts
            WHERE id = ?
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(row) => Ok(Some(self.row_to_artifact(row)?)),
            None => Ok(None),
        }
    }

    pub async fn list_project_artifacts(&self, project_id: &str) -> Result<Vec<Artifact>> {
        let rows = sqlx::query(
            r#"
            SELECT id, project_id, name, type, content, version, created_at, updated_at
            FROM artifacts
            WHERE project_id = ?
            ORDER BY updated_at DESC
            "#,
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|row| self.row_to_artifact(row))
            .collect()
    }

    pub async fn update_artifact(
        &self,
        id: &str,
        name: Option<String>,
        content: Option<String>,
    ) -> Result<Option<Artifact>> {
        // First check if artifact exists
        let existing = self.get_artifact(id).await?;
        if existing.is_none() {
            return Ok(None);
        }

        let mut artifact = existing.unwrap();

        // Update fields if provided
        if let Some(n) = name {
            artifact.name = n;
        }
        if content.is_some() {
            artifact.content = content;
            artifact.version += 1; // Increment version on content change
        }

        artifact.updated_at = Utc::now();

        sqlx::query(
            r#"
            UPDATE artifacts
            SET name = ?, content = ?, version = ?, updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(&artifact.name)
        .bind(&artifact.content)
        .bind(artifact.version)
        .bind(artifact.updated_at.naive_utc())
        .bind(id)
        .execute(&self.pool)
        .await?;

        Ok(Some(artifact))
    }

    pub async fn delete_artifact(&self, id: &str) -> Result<bool> {
        let result = sqlx::query("DELETE FROM artifacts WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected() > 0)
    }

    // Helper methods

    fn row_to_project(&self, row: sqlx::sqlite::SqliteRow) -> Result<Project> {
        let id: String = row.get("id");
        let name: String = row.get("name");
        let description: Option<String> = row.get("description");
        let tags_json: Option<String> = row.get("tags");
        let owner: Option<String> = row.get("owner");
        let created_at: NaiveDateTime = row.get("created_at");
        let updated_at: NaiveDateTime = row.get("updated_at");

        let tags = tags_json
            .as_ref()
            .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok());

        Ok(Project {
            id,
            name,
            description,
            tags,
            owner,
            created_at: Utc.from_utc_datetime(&created_at),
            updated_at: Utc.from_utc_datetime(&updated_at),
        })
    }

    fn row_to_artifact(&self, row: sqlx::sqlite::SqliteRow) -> Result<Artifact> {
        let id: String = row.get("id");
        let project_id: String = row.get("project_id");
        let name: String = row.get("name");
        let artifact_type_str: String = row.get("type");
        let content: Option<String> = row.get("content");
        let version: i32 = row.get("version");
        let created_at: NaiveDateTime = row.get("created_at");
        let updated_at: NaiveDateTime = row.get("updated_at");

        let artifact_type = artifact_type_str
            .parse::<ArtifactType>()
            .map_err(|e| anyhow::anyhow!(e))?;

        Ok(Artifact {
            id,
            project_id,
            name,
            artifact_type,
            content,
            version,
            created_at: Utc.from_utc_datetime(&created_at),
            updated_at: Utc.from_utc_datetime(&updated_at),
        })
    }
}
