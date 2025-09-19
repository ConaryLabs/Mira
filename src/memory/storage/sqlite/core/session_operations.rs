use anyhow::Result;
use chrono::{Duration, Utc};
use sqlx::SqlitePool;
use tracing::debug;

/// Handles session management operations
pub struct SessionOperations {
    pool: SqlitePool,
}

impl SessionOperations {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Get active sessions from the last N hours
    pub async fn get_active_sessions(&self, hours: i64) -> Result<Vec<String>> {
        let since = Utc::now() - Duration::hours(hours);
        let since_naive = since.naive_utc();
        
        let sessions: Vec<String> = sqlx::query_scalar(
            r#"
            SELECT DISTINCT session_id
            FROM memory_entries
            WHERE timestamp > ?
            ORDER BY timestamp DESC
            LIMIT 100
            "#
        )
        .bind(since_naive)
        .fetch_all(&self.pool)
        .await?;
        
        debug!("Found {} active sessions in last {} hours", sessions.len(), hours);
        Ok(sessions)
    }

    /// Update pin status of memories (placeholder - schema needs pin column)
    pub async fn update_pin_status(&self, memory_id: i64, _pinned: bool) -> Result<()> {
        // Note: pinned column doesn't exist in memory_entries yet
        // This would need to be added to the schema if needed
        debug!("Pin status update requested for memory {} (not implemented)", memory_id);
        Ok(())
    }

    /// Clean up old or inactive sessions (future implementation)
    pub async fn cleanup_old_sessions(&self, max_age_hours: i64) -> Result<usize> {
        let cutoff = Utc::now() - Duration::hours(max_age_hours);
        let cutoff_naive = cutoff.naive_utc();
        
        let result = sqlx::query(
            r#"
            DELETE FROM memory_entries 
            WHERE timestamp < ?
            "#
        )
        .bind(cutoff_naive)
        .execute(&self.pool)
        .await?;
        
        let deleted_count = result.rows_affected() as usize;
        debug!("Cleaned up {} old memory entries older than {} hours", deleted_count, max_age_hours);
        
        Ok(deleted_count)
    }
}
