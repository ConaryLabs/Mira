// backend/src/terminal/store.rs

use super::types::*;
use sqlx::SqlitePool;
use std::sync::Arc;
use tracing::{debug, info};

/// Manages terminal session persistence in SQLite
pub struct TerminalStore {
    pool: Arc<SqlitePool>,
}

impl TerminalStore {
    /// Create a new terminal store
    pub fn new(pool: Arc<SqlitePool>) -> Self {
        Self { pool }
    }

    /// Save a new terminal session to database
    pub async fn create_session(&self, info: &TerminalSessionInfo) -> TerminalResult<()> {
        info!("Saving terminal session: {} for project: {}", info.id, info.project_id);

        let created_at = info.created_at.timestamp();
        let closed_at = info.closed_at.map(|t| t.timestamp());

        sqlx::query!(
            r#"
            INSERT INTO terminal_sessions (
                id, project_id, conversation_session_id, working_directory,
                shell, created_at, closed_at, exit_code
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
            info.id,
            info.project_id,
            info.conversation_session_id,
            info.working_directory,
            info.shell,
            created_at,
            closed_at,
            info.exit_code
        )
        .execute(self.pool.as_ref())
        .await
        .map_err(|e| TerminalError::IoError(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Failed to create terminal session: {}", e)
        )))?;

        Ok(())
    }

    /// Get a terminal session by ID
    pub async fn get_session(&self, session_id: &str) -> TerminalResult<Option<TerminalSessionInfo>> {
        debug!("Fetching terminal session: {}", session_id);

        let row = sqlx::query!(
            r#"
            SELECT id as "id!", project_id as "project_id!",
                   conversation_session_id, working_directory as "working_directory!",
                   shell, created_at as "created_at!", closed_at, exit_code
            FROM terminal_sessions
            WHERE id = ?
            "#,
            session_id
        )
        .fetch_optional(self.pool.as_ref())
        .await
        .map_err(|e| TerminalError::IoError(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Failed to fetch terminal session: {}", e)
        )))?;

        Ok(row.map(|r| TerminalSessionInfo {
            id: r.id,
            project_id: r.project_id,
            conversation_session_id: r.conversation_session_id,
            working_directory: r.working_directory,
            shell: r.shell,
            created_at: chrono::DateTime::from_timestamp(r.created_at, 0)
                .unwrap_or_else(|| chrono::Utc::now()),
            closed_at: r.closed_at
                .and_then(|t| chrono::DateTime::from_timestamp(t, 0)),
            exit_code: r.exit_code.map(|c| c as i32),
        }))
    }

    /// List terminal sessions for a project
    pub async fn list_project_sessions(
        &self,
        project_id: &str,
        limit: Option<i64>,
    ) -> TerminalResult<Vec<TerminalSessionInfo>> {
        debug!("Listing terminal sessions for project: {}", project_id);

        let limit = limit.unwrap_or(100);

        let rows = sqlx::query!(
            r#"
            SELECT id as "id!", project_id as "project_id!",
                   conversation_session_id, working_directory as "working_directory!",
                   shell, created_at as "created_at!", closed_at, exit_code
            FROM terminal_sessions
            WHERE project_id = ?
            ORDER BY created_at DESC
            LIMIT ?
            "#,
            project_id,
            limit
        )
        .fetch_all(self.pool.as_ref())
        .await
        .map_err(|e| TerminalError::IoError(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Failed to list terminal sessions: {}", e)
        )))?;

        Ok(rows
            .into_iter()
            .map(|r| TerminalSessionInfo {
                id: r.id,
                project_id: r.project_id,
                conversation_session_id: r.conversation_session_id,
                working_directory: r.working_directory,
                shell: r.shell,
                created_at: chrono::DateTime::from_timestamp(r.created_at, 0)
                    .unwrap_or_else(|| chrono::Utc::now()),
                closed_at: r.closed_at
                    .and_then(|t| chrono::DateTime::from_timestamp(t, 0)),
                exit_code: r.exit_code.map(|c| c as i32),
            })
            .collect())
    }

    /// List active (not closed) terminal sessions for a project
    pub async fn list_active_sessions(&self, project_id: &str) -> TerminalResult<Vec<TerminalSessionInfo>> {
        debug!("Listing active terminal sessions for project: {}", project_id);

        let rows = sqlx::query!(
            r#"
            SELECT id as "id!", project_id as "project_id!",
                   conversation_session_id, working_directory as "working_directory!",
                   shell, created_at as "created_at!", closed_at, exit_code
            FROM terminal_sessions
            WHERE project_id = ? AND closed_at IS NULL
            ORDER BY created_at DESC
            "#,
            project_id
        )
        .fetch_all(self.pool.as_ref())
        .await
        .map_err(|e| TerminalError::IoError(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Failed to list active sessions: {}", e)
        )))?;

        Ok(rows
            .into_iter()
            .map(|r| TerminalSessionInfo {
                id: r.id,
                project_id: r.project_id,
                conversation_session_id: r.conversation_session_id,
                working_directory: r.working_directory,
                shell: r.shell,
                created_at: chrono::DateTime::from_timestamp(r.created_at, 0)
                    .unwrap_or_else(|| chrono::Utc::now()),
                closed_at: r.closed_at
                    .and_then(|t| chrono::DateTime::from_timestamp(t, 0)),
                exit_code: r.exit_code.map(|c| c as i32),
            })
            .collect())
    }

    /// Close a terminal session
    pub async fn close_session(&self, session_id: &str, exit_code: Option<i32>) -> TerminalResult<()> {
        info!("Closing terminal session: {} with exit code: {:?}", session_id, exit_code);

        let now = chrono::Utc::now().timestamp();

        sqlx::query!(
            r#"
            UPDATE terminal_sessions
            SET closed_at = ?, exit_code = ?
            WHERE id = ?
            "#,
            now,
            exit_code,
            session_id
        )
        .execute(self.pool.as_ref())
        .await
        .map_err(|e| TerminalError::IoError(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Failed to close terminal session: {}", e)
        )))?;

        Ok(())
    }

    /// Delete a terminal session
    pub async fn delete_session(&self, session_id: &str) -> TerminalResult<()> {
        info!("Deleting terminal session: {}", session_id);

        sqlx::query!(
            r#"
            DELETE FROM terminal_sessions
            WHERE id = ?
            "#,
            session_id
        )
        .execute(self.pool.as_ref())
        .await
        .map_err(|e| TerminalError::IoError(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Failed to delete terminal session: {}", e)
        )))?;

        Ok(())
    }

    /// Delete all terminal sessions for a project
    pub async fn delete_project_sessions(&self, project_id: &str) -> TerminalResult<()> {
        info!("Deleting all terminal sessions for project: {}", project_id);

        sqlx::query!(
            r#"
            DELETE FROM terminal_sessions
            WHERE project_id = ?
            "#,
            project_id
        )
        .execute(self.pool.as_ref())
        .await
        .map_err(|e| TerminalError::IoError(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Failed to delete project sessions: {}", e)
        )))?;

        Ok(())
    }
}
