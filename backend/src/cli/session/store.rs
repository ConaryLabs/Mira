// backend/src/cli/session/store.rs
// SQLite-based session store for CLI state

use anyhow::{Context, Result};
use chrono::Utc;

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
    async fn test_preferences() {
        let (store, _temp) = create_test_store().await;

        store.set_preference("test_key", "test_value").await.unwrap();
        let value = store.get_preference("test_key").await.unwrap();
        assert_eq!(value, Some("test_value".to_string()));
    }
}
