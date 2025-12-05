// backend/src/cli/session/store.rs
// SQLite-based session store for CLI state

use anyhow::{Context, Result};
use chrono::{TimeZone, Utc};
use std::path::PathBuf;

use super::types::{CliSession, SessionFilter};
use crate::cli::config::CliConfig;

/// Session store backed by SQLite
pub struct SessionStore {
    pool: sqlx::SqlitePool,
}

impl SessionStore {
    /// Create a new session store, initializing the database if needed
    pub async fn new() -> Result<Self> {
        let db_path = CliConfig::cli_db_path()?;

        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory: {:?}", parent))?;
        }

        // Connect to database
        let db_url = format!("sqlite://{}?mode=rwc", db_path.display());
        let pool = sqlx::SqlitePool::connect(&db_url)
            .await
            .with_context(|| format!("Failed to connect to CLI database: {}", db_path.display()))?;

        // Initialize schema
        Self::init_schema(&pool).await?;

        Ok(Self { pool })
    }

    /// Initialize the database schema
    async fn init_schema(pool: &sqlx::SqlitePool) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS cli_sessions (
                id TEXT PRIMARY KEY,
                name TEXT,
                project_path TEXT,
                backend_session_id TEXT NOT NULL,
                last_message TEXT,
                message_count INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL,
                last_active INTEGER NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_sessions_last_active
            ON cli_sessions(last_active DESC);

            CREATE INDEX IF NOT EXISTS idx_sessions_project_path
            ON cli_sessions(project_path);

            CREATE TABLE IF NOT EXISTS cli_preferences (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at INTEGER NOT NULL
            );
            "#,
        )
        .execute(pool)
        .await
        .context("Failed to initialize CLI database schema")?;

        Ok(())
    }

    /// Save a session (insert or update)
    /// DEPRECATED: This method is no longer used - sessions are managed via WebSocket API
    #[allow(dead_code)]
    pub async fn save(&self, session: &CliSession) -> Result<()> {
        let project_path = session.project_path.as_ref().map(|p| p.to_string_lossy().to_string());
        let created_at = session.created_at.timestamp();
        let last_active = session.last_active.timestamp();

        sqlx::query(
            r#"
            INSERT INTO cli_sessions (id, name, project_path, last_message, message_count, created_at, last_active)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                project_path = excluded.project_path,
                last_message = excluded.last_message,
                message_count = excluded.message_count,
                last_active = excluded.last_active
            "#,
        )
        .bind(&session.id)
        .bind(&session.name)
        .bind(&project_path)
        .bind(&session.last_message)
        .bind(session.message_count as i64)
        .bind(created_at)
        .bind(last_active)
        .execute(&self.pool)
        .await
        .context("Failed to save session")?;

        Ok(())
    }

    /// Get a session by ID
    /// DEPRECATED: Use MiraClient::get_session instead
    #[allow(dead_code)]
    pub async fn get(&self, id: &str) -> Result<Option<CliSession>> {
        let row = sqlx::query_as::<_, SessionRow>(
            "SELECT id, name, project_path, last_message, message_count, created_at, last_active FROM cli_sessions WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to fetch session")?;

        Ok(row.map(|r| r.into()))
    }

    /// Get the most recent session
    /// DEPRECATED: Use MiraClient::list_sessions instead
    #[allow(dead_code)]
    pub async fn get_most_recent(&self) -> Result<Option<CliSession>> {
        let row = sqlx::query_as::<_, SessionRow>(
            "SELECT id, name, project_path, last_message, message_count, created_at, last_active FROM cli_sessions ORDER BY last_active DESC LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await
        .context("Failed to fetch most recent session")?;

        Ok(row.map(|r| r.into()))
    }

    /// Get the most recent session for a specific project
    /// DEPRECATED: Use MiraClient::list_sessions with project_path filter
    #[allow(dead_code)]
    pub async fn get_most_recent_for_project(&self, project_path: &PathBuf) -> Result<Option<CliSession>> {
        let path_str = project_path.to_string_lossy().to_string();
        let row = sqlx::query_as::<_, SessionRow>(
            "SELECT id, name, project_path, last_message, message_count, created_at, last_active FROM cli_sessions WHERE project_path = ? ORDER BY last_active DESC LIMIT 1",
        )
        .bind(&path_str)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to fetch most recent session for project")?;

        Ok(row.map(|r| r.into()))
    }

    /// List sessions with optional filtering
    /// DEPRECATED: Use MiraClient::list_sessions instead
    #[allow(dead_code)]
    pub async fn list(&self, filter: SessionFilter) -> Result<Vec<CliSession>> {
        let limit = filter.limit.unwrap_or(50) as i64;

        let rows = if let Some(ref project_path) = filter.project_path {
            let path_str = project_path.to_string_lossy().to_string();
            sqlx::query_as::<_, SessionRow>(
                "SELECT id, name, project_path, last_message, message_count, created_at, last_active FROM cli_sessions WHERE project_path = ? ORDER BY last_active DESC LIMIT ?",
            )
            .bind(&path_str)
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .context("Failed to list sessions")?
        } else if let Some(ref search) = filter.search {
            let search_pattern = format!("%{}%", search);
            sqlx::query_as::<_, SessionRow>(
                "SELECT id, name, project_path, last_message, message_count, created_at, last_active FROM cli_sessions WHERE name LIKE ? OR last_message LIKE ? ORDER BY last_active DESC LIMIT ?",
            )
            .bind(&search_pattern)
            .bind(&search_pattern)
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .context("Failed to list sessions")?
        } else {
            sqlx::query_as::<_, SessionRow>(
                "SELECT id, name, project_path, last_message, message_count, created_at, last_active FROM cli_sessions ORDER BY last_active DESC LIMIT ?",
            )
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .context("Failed to list sessions")?
        };

        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    /// Delete a session
    pub async fn delete(&self, id: &str) -> Result<bool> {
        let result = sqlx::query("DELETE FROM cli_sessions WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .context("Failed to delete session")?;

        Ok(result.rows_affected() > 0)
    }

    /// Get session count
    pub async fn count(&self) -> Result<u64> {
        let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM cli_sessions")
            .fetch_one(&self.pool)
            .await
            .context("Failed to count sessions")?;

        Ok(row.0 as u64)
    }

    /// Save a preference
    pub async fn set_preference(&self, key: &str, value: &str) -> Result<()> {
        let now = Utc::now().timestamp();
        sqlx::query(
            "INSERT INTO cli_preferences (key, value, updated_at) VALUES (?, ?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        )
        .bind(key)
        .bind(value)
        .bind(now)
        .execute(&self.pool)
        .await
        .context("Failed to save preference")?;

        Ok(())
    }

    /// Get a preference
    pub async fn get_preference(&self, key: &str) -> Result<Option<String>> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT value FROM cli_preferences WHERE key = ?",
        )
        .bind(key)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to get preference")?;

        Ok(row.map(|r| r.0))
    }
}

/// Database row representation
/// DEPRECATED: This is for the old local CLI session store
#[derive(sqlx::FromRow)]
struct SessionRow {
    id: String,
    name: Option<String>,
    project_path: Option<String>,
    last_message: Option<String>,
    message_count: i64,
    created_at: i64,
    last_active: i64,
}

impl From<SessionRow> for CliSession {
    fn from(row: SessionRow) -> Self {
        Self {
            id: row.id,
            name: row.name,
            project_path: row.project_path.map(PathBuf::from),
            last_message: row.last_message,
            message_count: row.message_count as u32,
            created_at: Utc.timestamp_opt(row.created_at, 0).unwrap(),
            last_active: Utc.timestamp_opt(row.last_active, 0).unwrap(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn create_test_store() -> (SessionStore, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test_cli.db");

        // Set up test database URL
        let db_url = format!("sqlite://{}?mode=rwc", db_path.display());
        let pool = sqlx::SqlitePool::connect(&db_url).await.unwrap();
        SessionStore::init_schema(&pool).await.unwrap();

        (SessionStore { pool }, temp_dir)
    }

    #[tokio::test]
    async fn test_save_and_get_session() {
        let (store, _temp) = create_test_store().await;

        let session = CliSession::new("backend-123".to_string(), None);
        store.save(&session).await.unwrap();

        let loaded = store.get(&session.id).await.unwrap();
        assert!(loaded.is_some());
        let loaded = loaded.unwrap();
        assert_eq!(loaded.id, "backend-123");
    }

    #[tokio::test]
    async fn test_get_most_recent() {
        let (store, _temp) = create_test_store().await;

        let session1 = CliSession::new("backend-1".to_string(), None);
        store.save(&session1).await.unwrap();

        // Wait a bit to ensure different timestamps
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        let session2 = CliSession::new("backend-2".to_string(), None);
        store.save(&session2).await.unwrap();

        let most_recent = store.get_most_recent().await.unwrap();
        assert!(most_recent.is_some());
        assert_eq!(most_recent.unwrap().id, "backend-2");
    }

    #[tokio::test]
    async fn test_list_sessions() {
        let (store, _temp) = create_test_store().await;

        for i in 0..5 {
            let session = CliSession::new(format!("backend-{}", i), None);
            store.save(&session).await.unwrap();
        }

        let sessions = store.list(SessionFilter::new().with_limit(3)).await.unwrap();
        assert_eq!(sessions.len(), 3);
    }

    #[tokio::test]
    async fn test_preferences() {
        let (store, _temp) = create_test_store().await;

        store.set_preference("test_key", "test_value").await.unwrap();
        let value = store.get_preference("test_key").await.unwrap();
        assert_eq!(value, Some("test_value".to_string()));
    }
}
