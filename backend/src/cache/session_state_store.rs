// backend/src/cache/session_state_store.rs
// SQLite persistence for session cache state
//
// Stores session-level cache tracking to enable incremental context updates
// and cache hit rate monitoring across sessions.

use anyhow::Result;
use chrono::{TimeZone, Utc};
use sqlx::{Row, SqlitePool};
use tracing::debug;

use super::session_state::{ContextHashes, FileContentHash, SessionCacheState};

/// SQLite storage for session cache state
pub struct SessionCacheStore {
    db: SqlitePool,
}

impl SessionCacheStore {
    /// Create a new session cache store
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }

    /// Get cache state for a session
    pub async fn get(&self, session_id: &str) -> Result<Option<SessionCacheState>> {
        let row = sqlx::query(
            r#"
            SELECT session_id, static_prefix_hash, last_call_at,
                   project_context_hash, memory_context_hash,
                   code_intelligence_hash, file_context_hash,
                   static_prefix_tokens, last_cached_tokens,
                   total_requests, total_cached_tokens
            FROM session_cache_state
            WHERE session_id = ?
            "#,
        )
        .bind(session_id)
        .fetch_optional(&self.db)
        .await?;

        let Some(row) = row else {
            return Ok(None);
        };

        let session_id: String = row.get("session_id");
        let static_prefix_hash: String = row.get("static_prefix_hash");
        let last_call_at: i64 = row.get("last_call_at");
        let project_context_hash: Option<String> = row.get("project_context_hash");
        let memory_context_hash: Option<String> = row.get("memory_context_hash");
        let code_intelligence_hash: Option<String> = row.get("code_intelligence_hash");
        let file_context_hash: Option<String> = row.get("file_context_hash");
        let static_prefix_tokens: i64 = row.get("static_prefix_tokens");
        let last_cached_tokens: i64 = row.get("last_cached_tokens");
        let total_requests: i64 = row.get("total_requests");
        let total_cached_tokens: i64 = row.get("total_cached_tokens");

        // Load file hashes
        let file_rows = sqlx::query(
            r#"
            SELECT file_path, content_hash, token_estimate, sent_at
            FROM session_file_hashes
            WHERE session_id = ?
            "#,
        )
        .bind(&session_id)
        .fetch_all(&self.db)
        .await?;

        let mut file_contents = std::collections::HashMap::new();
        for file_row in file_rows {
            let path: String = file_row.get("file_path");
            let content_hash: String = file_row.get("content_hash");
            let token_estimate: i64 = file_row.get("token_estimate");
            let sent_at: i64 = file_row.get("sent_at");

            file_contents.insert(
                path.clone(),
                FileContentHash {
                    path,
                    content_hash,
                    token_estimate,
                    sent_at: Utc.timestamp_opt(sent_at, 0).unwrap(),
                },
            );
        }

        let context_hashes = ContextHashes {
            project_context: project_context_hash,
            memory_context: memory_context_hash,
            code_intelligence: code_intelligence_hash,
            file_context: file_context_hash,
            file_contents,
        };

        Ok(Some(SessionCacheState {
            session_id,
            static_prefix_hash,
            last_call_at: Utc.timestamp_opt(last_call_at, 0).unwrap(),
            context_hashes,
            static_prefix_tokens,
            last_reported_cached_tokens: last_cached_tokens,
            total_requests,
            total_cached_tokens,
        }))
    }

    /// Update or create cache state for a session
    pub async fn upsert(&self, state: &SessionCacheState) -> Result<()> {
        let now = Utc::now().timestamp();

        // Upsert main state
        sqlx::query(
            r#"
            INSERT INTO session_cache_state (
                session_id, static_prefix_hash, last_call_at,
                project_context_hash, memory_context_hash,
                code_intelligence_hash, file_context_hash,
                static_prefix_tokens, last_cached_tokens,
                total_requests, total_cached_tokens,
                created_at, updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(session_id) DO UPDATE SET
                static_prefix_hash = excluded.static_prefix_hash,
                last_call_at = excluded.last_call_at,
                project_context_hash = excluded.project_context_hash,
                memory_context_hash = excluded.memory_context_hash,
                code_intelligence_hash = excluded.code_intelligence_hash,
                file_context_hash = excluded.file_context_hash,
                static_prefix_tokens = excluded.static_prefix_tokens,
                last_cached_tokens = excluded.last_cached_tokens,
                total_requests = excluded.total_requests,
                total_cached_tokens = excluded.total_cached_tokens,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(&state.session_id)
        .bind(&state.static_prefix_hash)
        .bind(state.last_call_at.timestamp())
        .bind(&state.context_hashes.project_context)
        .bind(&state.context_hashes.memory_context)
        .bind(&state.context_hashes.code_intelligence)
        .bind(&state.context_hashes.file_context)
        .bind(state.static_prefix_tokens)
        .bind(state.last_reported_cached_tokens)
        .bind(state.total_requests)
        .bind(state.total_cached_tokens)
        .bind(now)
        .bind(now)
        .execute(&self.db)
        .await?;

        // Update file hashes (delete old, insert new)
        sqlx::query("DELETE FROM session_file_hashes WHERE session_id = ?")
            .bind(&state.session_id)
            .execute(&self.db)
            .await?;

        for file_hash in state.context_hashes.file_contents.values() {
            sqlx::query(
                r#"
                INSERT INTO session_file_hashes (session_id, file_path, content_hash, token_estimate, sent_at)
                VALUES (?, ?, ?, ?, ?)
                "#,
            )
            .bind(&state.session_id)
            .bind(&file_hash.path)
            .bind(&file_hash.content_hash)
            .bind(file_hash.token_estimate)
            .bind(file_hash.sent_at.timestamp())
            .execute(&self.db)
            .await?;
        }

        debug!(
            "Updated session cache state for {}: {} cached tokens, {:.1}% hit rate",
            state.session_id,
            state.total_cached_tokens,
            state.cache_hit_rate() * 100.0
        );

        Ok(())
    }

    /// Invalidate cache state for a session (e.g., when static prefix changes)
    pub async fn invalidate(&self, session_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM session_file_hashes WHERE session_id = ?")
            .bind(session_id)
            .execute(&self.db)
            .await?;

        sqlx::query("DELETE FROM session_cache_state WHERE session_id = ?")
            .bind(session_id)
            .execute(&self.db)
            .await?;

        debug!("Invalidated session cache state for {}", session_id);
        Ok(())
    }

    /// Clean up old session cache states (older than specified hours)
    pub async fn cleanup_old_states(&self, max_age_hours: i64) -> Result<u64> {
        let cutoff = Utc::now().timestamp() - (max_age_hours * 3600);

        // Delete file hashes for old sessions
        sqlx::query(
            r#"
            DELETE FROM session_file_hashes
            WHERE session_id IN (
                SELECT session_id FROM session_cache_state WHERE last_call_at < ?
            )
            "#,
        )
        .bind(cutoff)
        .execute(&self.db)
        .await?;

        // Delete old sessions
        let result = sqlx::query("DELETE FROM session_cache_state WHERE last_call_at < ?")
            .bind(cutoff)
            .execute(&self.db)
            .await?;

        let deleted = result.rows_affected();
        if deleted > 0 {
            debug!(
                "Cleaned up {} old session cache states (older than {} hours)",
                deleted, max_age_hours
            );
        }

        Ok(deleted)
    }

    /// Get aggregate cache statistics across all sessions
    pub async fn get_aggregate_stats(&self) -> Result<CacheAggregateStats> {
        let row = sqlx::query(
            r#"
            SELECT
                COUNT(*) as total_sessions,
                COALESCE(SUM(total_requests), 0) as total_requests,
                COALESCE(SUM(total_cached_tokens), 0) as total_cached_tokens,
                COALESCE(AVG(static_prefix_tokens), 0) as avg_prefix_tokens
            FROM session_cache_state
            "#,
        )
        .fetch_one(&self.db)
        .await?;

        Ok(CacheAggregateStats {
            total_sessions: row.get("total_sessions"),
            total_requests: row.get("total_requests"),
            total_cached_tokens: row.get("total_cached_tokens"),
            avg_prefix_tokens: row.get("avg_prefix_tokens"),
        })
    }
}

/// Aggregate cache statistics
#[derive(Debug, Clone)]
pub struct CacheAggregateStats {
    pub total_sessions: i64,
    pub total_requests: i64,
    pub total_cached_tokens: i64,
    pub avg_prefix_tokens: f64,
}

impl CacheAggregateStats {
    /// Estimate overall cache hit rate
    pub fn estimated_hit_rate(&self) -> f64 {
        if self.total_requests == 0 || self.avg_prefix_tokens <= 0.0 {
            return 0.0;
        }
        let expected_cached = self.total_requests as f64 * self.avg_prefix_tokens;
        (self.total_cached_tokens as f64 / expected_cached).min(1.0)
    }
}
