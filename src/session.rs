// src/session.rs

use sqlx::{SqlitePool, Row};
use uuid::Uuid;

pub struct SessionStore {
    pool: SqlitePool,
}

impl SessionStore {
    pub async fn new(db_path: &str) -> anyhow::Result<Self> {
        let pool = SqlitePool::connect(db_path).await?;
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS chat_history (
                session_id TEXT,
                role TEXT,
                content TEXT,
                ts INTEGER DEFAULT (strftime('%s','now'))
            )
            "#,
        )
        .execute(&pool)
        .await?;
        Ok(SessionStore { pool })
    }

    pub async fn save_message(&self, session_id: &str, role: &str, content: &str) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO chat_history (session_id, role, content) VALUES (?, ?, ?)",
        )
        .bind(session_id)
        .bind(role)
        .bind(content)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn load_history(&self, session_id: &str, limit: usize) -> anyhow::Result<Vec<(String, String)>> {
        let rows = sqlx::query(
            "SELECT role, content FROM chat_history WHERE session_id = ? ORDER BY ts DESC LIMIT ?",
        )
        .bind(session_id)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        // Reverse to get oldest->newest order
        Ok(rows.into_iter()
            .map(|row| (row.get::<String, _>(0), row.get::<String, _>(1)))
            .rev()
            .collect())
    }
}

/// Generates a new random session ID (UUID v4)
pub fn generate_session_id() -> String {
    Uuid::new_v4().to_string()
}
