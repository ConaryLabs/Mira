// backend/src/project/guidelines.rs
// Project guidelines storage and retrieval

use anyhow::Result;
use chrono::Utc;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

/// Project guidelines - stores and retrieves README/guidelines files for projects
pub struct ProjectGuidelinesService {
    pool: SqlitePool,
}

/// Project guidelines entry
#[derive(Debug, Clone)]
pub struct ProjectGuidelines {
    pub id: i64,
    pub project_id: String,
    pub file_path: String,
    pub content: String,
    pub content_hash: String,
    pub last_loaded: i64,
}

impl ProjectGuidelinesService {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Store or update project guidelines
    pub async fn save_guidelines(
        &self,
        project_id: &str,
        file_path: &str,
        content: &str,
    ) -> Result<i64> {
        let now = Utc::now().timestamp();
        let content_hash = self.hash_content(content);

        // Check if guidelines already exist for this project
        let existing = sqlx::query!(
            r#"SELECT id, content_hash FROM project_guidelines WHERE project_id = ?"#,
            project_id
        )
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = existing {
            // Update if content changed
            if row.content_hash != content_hash {
                sqlx::query!(
                    r#"
                    UPDATE project_guidelines
                    SET file_path = ?, content = ?, content_hash = ?, last_loaded = ?, updated_at = ?
                    WHERE id = ?
                    "#,
                    file_path,
                    content,
                    content_hash,
                    now,
                    now,
                    row.id
                )
                .execute(&self.pool)
                .await?;
            } else {
                // Just update last_loaded
                sqlx::query!(
                    r#"UPDATE project_guidelines SET last_loaded = ? WHERE id = ?"#,
                    now,
                    row.id
                )
                .execute(&self.pool)
                .await?;
            }
            Ok(row.id.unwrap_or(0))
        } else {
            // Insert new
            let id = sqlx::query!(
                r#"
                INSERT INTO project_guidelines (project_id, file_path, content, content_hash, last_loaded, created_at, updated_at)
                VALUES (?, ?, ?, ?, ?, ?, ?)
                "#,
                project_id,
                file_path,
                content,
                content_hash,
                now,
                now,
                now
            )
            .execute(&self.pool)
            .await?
            .last_insert_rowid();

            Ok(id)
        }
    }

    /// Get guidelines for a project
    pub async fn get_guidelines(&self, project_id: &str) -> Result<Option<ProjectGuidelines>> {
        let row = sqlx::query!(
            r#"
            SELECT id, project_id, file_path, content, content_hash, last_loaded
            FROM project_guidelines
            WHERE project_id = ?
            "#,
            project_id
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| ProjectGuidelines {
            id: r.id.unwrap_or(0),
            project_id: r.project_id,
            file_path: r.file_path,
            content: r.content,
            content_hash: r.content_hash,
            last_loaded: r.last_loaded,
        }))
    }

    /// Delete guidelines for a project
    pub async fn delete_guidelines(&self, project_id: &str) -> Result<bool> {
        let result = sqlx::query!(
            r#"DELETE FROM project_guidelines WHERE project_id = ?"#,
            project_id
        )
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    /// Check if guidelines exist and are up to date
    pub async fn is_up_to_date(&self, project_id: &str, content: &str) -> Result<bool> {
        let content_hash = self.hash_content(content);

        let row = sqlx::query!(
            r#"SELECT content_hash FROM project_guidelines WHERE project_id = ?"#,
            project_id
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| r.content_hash == content_hash).unwrap_or(false))
    }

    /// Hash content for comparison
    fn hash_content(&self, content: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// Get guidelines content formatted for LLM context
    pub async fn get_guidelines_for_context(
        &self,
        project_id: &str,
    ) -> Result<Option<String>> {
        let guidelines = self.get_guidelines(project_id).await?;

        Ok(guidelines.map(|g| {
            format!(
                "## Project Guidelines (from {})\n\n{}",
                g.file_path, g.content
            )
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn setup_test_db() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .unwrap();

        sqlx::query(
            r#"
            CREATE TABLE project_guidelines (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id TEXT NOT NULL UNIQUE,
                file_path TEXT NOT NULL,
                content TEXT NOT NULL,
                content_hash TEXT NOT NULL,
                last_loaded INTEGER NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        pool
    }

    #[tokio::test]
    async fn test_save_and_get_guidelines() {
        let pool = setup_test_db().await;
        let service = ProjectGuidelinesService::new(pool);

        let project_id = "test-project";
        let file_path = "README.md";
        let content = "# Test Project\n\nThis is a test project.";

        // Save guidelines
        let id = service
            .save_guidelines(project_id, file_path, content)
            .await
            .unwrap();
        assert!(id > 0);

        // Get guidelines
        let guidelines = service.get_guidelines(project_id).await.unwrap();
        assert!(guidelines.is_some());
        let g = guidelines.unwrap();
        assert_eq!(g.project_id, project_id);
        assert_eq!(g.file_path, file_path);
        assert_eq!(g.content, content);
    }

    #[tokio::test]
    async fn test_update_guidelines() {
        let pool = setup_test_db().await;
        let service = ProjectGuidelinesService::new(pool);

        let project_id = "test-project";
        let file_path = "README.md";
        let content1 = "# Test Project v1";
        let content2 = "# Test Project v2";

        // Save initial
        service
            .save_guidelines(project_id, file_path, content1)
            .await
            .unwrap();

        // Update
        service
            .save_guidelines(project_id, file_path, content2)
            .await
            .unwrap();

        // Verify update
        let guidelines = service.get_guidelines(project_id).await.unwrap().unwrap();
        assert_eq!(guidelines.content, content2);
    }

    #[tokio::test]
    async fn test_is_up_to_date() {
        let pool = setup_test_db().await;
        let service = ProjectGuidelinesService::new(pool);

        let project_id = "test-project";
        let content = "# Test Project";

        // Before saving - not up to date
        assert!(!service.is_up_to_date(project_id, content).await.unwrap());

        // Save
        service
            .save_guidelines(project_id, "README.md", content)
            .await
            .unwrap();

        // Now it should be up to date
        assert!(service.is_up_to_date(project_id, content).await.unwrap());

        // Different content - not up to date
        assert!(!service
            .is_up_to_date(project_id, "Different content")
            .await
            .unwrap());
    }
}
