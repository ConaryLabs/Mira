// backend/src/session/injection.rs
// Handles injection of summaries and updates from Codex sessions into Voice sessions

use crate::session::types::*;
use anyhow::Result;
use sqlx::SqlitePool;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, info};

/// Service for managing injections from Codex to Voice sessions
pub struct InjectionService {
    pool: SqlitePool,
}

fn now_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl InjectionService {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Inject a Codex completion summary into a Voice session
    pub async fn inject_codex_completion(
        &self,
        voice_session_id: &str,
        codex_session_id: &str,
        summary: &str,
        metadata: CodexCompletionMetadata,
    ) -> Result<i64> {
        let now = now_timestamp();
        let metadata_json = serde_json::to_string(&metadata)?;

        // Get next sequence number for this target session
        let seq_result: Option<(i32,)> = sqlx::query_as(
            "SELECT COALESCE(MAX(sequence_num), 0) + 1 FROM session_injections WHERE target_session_id = ?",
        )
        .bind(voice_session_id)
        .fetch_optional(&self.pool)
        .await?;
        let sequence_num = seq_result.map(|(s,)| s).unwrap_or(1);

        let result = sqlx::query(
            r#"
            INSERT INTO session_injections (
                target_session_id, source_session_id, injection_type,
                content, metadata, injected_at, sequence_num
            )
            VALUES (?, ?, 'codex_completion', ?, ?, ?, ?)
            "#,
        )
        .bind(voice_session_id)
        .bind(codex_session_id)
        .bind(summary)
        .bind(&metadata_json)
        .bind(now)
        .bind(sequence_num)
        .execute(&self.pool)
        .await?;

        let injection_id = result.last_insert_rowid();

        info!(
            injection_id = injection_id,
            voice_session_id = %voice_session_id,
            codex_session_id = %codex_session_id,
            files_changed = metadata.files_changed.len(),
            "Injected Codex completion summary"
        );

        Ok(injection_id)
    }

    /// Inject a progress update from Codex session
    pub async fn inject_codex_progress(
        &self,
        voice_session_id: &str,
        codex_session_id: &str,
        progress_message: &str,
        current_activity: Option<&str>,
        progress_percent: Option<u8>,
    ) -> Result<i64> {
        let now = now_timestamp();
        let metadata = serde_json::json!({
            "current_activity": current_activity,
            "progress_percent": progress_percent,
        });

        let seq_result: Option<(i32,)> = sqlx::query_as(
            "SELECT COALESCE(MAX(sequence_num), 0) + 1 FROM session_injections WHERE target_session_id = ?",
        )
        .bind(voice_session_id)
        .fetch_optional(&self.pool)
        .await?;
        let sequence_num = seq_result.map(|(s,)| s).unwrap_or(1);

        let result = sqlx::query(
            r#"
            INSERT INTO session_injections (
                target_session_id, source_session_id, injection_type,
                content, metadata, injected_at, sequence_num
            )
            VALUES (?, ?, 'codex_progress', ?, ?, ?, ?)
            "#,
        )
        .bind(voice_session_id)
        .bind(codex_session_id)
        .bind(progress_message)
        .bind(metadata.to_string())
        .bind(now)
        .bind(sequence_num)
        .execute(&self.pool)
        .await?;

        let injection_id = result.last_insert_rowid();

        debug!(
            injection_id = injection_id,
            voice_session_id = %voice_session_id,
            codex_session_id = %codex_session_id,
            progress = progress_message,
            "Injected Codex progress update"
        );

        Ok(injection_id)
    }

    /// Inject an error notification from a failed Codex session
    pub async fn inject_codex_error(
        &self,
        voice_session_id: &str,
        codex_session_id: &str,
        error_message: &str,
        task_description: &str,
    ) -> Result<i64> {
        let now = now_timestamp();
        let metadata = serde_json::json!({
            "task_description": task_description,
            "error_type": "codex_failure",
        });

        let seq_result: Option<(i32,)> = sqlx::query_as(
            "SELECT COALESCE(MAX(sequence_num), 0) + 1 FROM session_injections WHERE target_session_id = ?",
        )
        .bind(voice_session_id)
        .fetch_optional(&self.pool)
        .await?;
        let sequence_num = seq_result.map(|(s,)| s).unwrap_or(1);

        let result = sqlx::query(
            r#"
            INSERT INTO session_injections (
                target_session_id, source_session_id, injection_type,
                content, metadata, injected_at, sequence_num
            )
            VALUES (?, ?, 'codex_error', ?, ?, ?, ?)
            "#,
        )
        .bind(voice_session_id)
        .bind(codex_session_id)
        .bind(error_message)
        .bind(metadata.to_string())
        .bind(now)
        .bind(sequence_num)
        .execute(&self.pool)
        .await?;

        let injection_id = result.last_insert_rowid();

        info!(
            injection_id = injection_id,
            voice_session_id = %voice_session_id,
            codex_session_id = %codex_session_id,
            error = %error_message,
            "Injected Codex error notification"
        );

        Ok(injection_id)
    }

    /// Get pending (unacknowledged) injections for a Voice session
    pub async fn get_pending_injections(
        &self,
        voice_session_id: &str,
    ) -> Result<Vec<SessionInjection>> {
        let rows: Vec<(
            i64,              // id
            String,           // target_session_id
            String,           // source_session_id
            String,           // injection_type
            String,           // content
            Option<String>,   // metadata
            i64,              // injected_at
            i64,              // acknowledged (as integer)
            Option<i64>,      // acknowledged_at
            i32,              // sequence_num
        )> = sqlx::query_as(
            r#"
            SELECT id, target_session_id, source_session_id, injection_type,
                   content, metadata, injected_at, acknowledged, acknowledged_at, sequence_num
            FROM session_injections
            WHERE target_session_id = ? AND acknowledged = 0
            ORDER BY sequence_num ASC
            "#,
        )
        .bind(voice_session_id)
        .fetch_all(&self.pool)
        .await?;

        let injections = rows
            .into_iter()
            .map(|(id, target, source, inj_type, content, metadata, injected_at, ack, ack_at, seq)| {
                SessionInjection {
                    id,
                    target_session_id: target,
                    source_session_id: source,
                    injection_type: InjectionType::from_str(&inj_type)
                        .unwrap_or(InjectionType::CodexCompletion),
                    content,
                    metadata: metadata.and_then(|m| serde_json::from_str(&m).ok()),
                    injected_at,
                    acknowledged: ack != 0,
                    acknowledged_at: ack_at,
                    sequence_num: seq,
                }
            })
            .collect();

        Ok(injections)
    }

    /// Acknowledge an injection (mark as seen by Voice session)
    pub async fn acknowledge_injection(&self, injection_id: i64) -> Result<()> {
        let now = now_timestamp();

        sqlx::query(
            "UPDATE session_injections SET acknowledged = 1, acknowledged_at = ? WHERE id = ?",
        )
        .bind(now)
        .bind(injection_id)
        .execute(&self.pool)
        .await?;

        debug!(injection_id = injection_id, "Acknowledged injection");
        Ok(())
    }

    /// Acknowledge all pending injections for a Voice session
    pub async fn acknowledge_all(&self, voice_session_id: &str) -> Result<u64> {
        let now = now_timestamp();

        let result = sqlx::query(
            r#"
            UPDATE session_injections
            SET acknowledged = 1, acknowledged_at = ?
            WHERE target_session_id = ? AND acknowledged = 0
            "#,
        )
        .bind(now)
        .bind(voice_session_id)
        .execute(&self.pool)
        .await?;

        let count = result.rows_affected();
        if count > 0 {
            debug!(
                voice_session_id = %voice_session_id,
                count = count,
                "Acknowledged all pending injections"
            );
        }

        Ok(count)
    }

    /// Format pending injections for inclusion in system prompt
    pub fn format_for_prompt(&self, injections: &[SessionInjection]) -> Option<String> {
        if injections.is_empty() {
            return None;
        }

        let mut parts = Vec::new();
        parts.push("## Background Work Updates\n".to_string());

        for inj in injections {
            match inj.injection_type {
                InjectionType::CodexCompletion => {
                    parts.push(format!(
                        "**Completed background task:**\n{}\n",
                        inj.content
                    ));

                    // Add file changes if present
                    if let Some(ref metadata) = inj.metadata {
                        if let Some(files) = metadata.get("files_changed").and_then(|v| v.as_array()) {
                            if !files.is_empty() {
                                let file_list: Vec<&str> = files
                                    .iter()
                                    .filter_map(|f| f.as_str())
                                    .take(10)
                                    .collect();
                                parts.push(format!("Files modified: {}\n", file_list.join(", ")));
                            }
                        }
                    }
                }
                InjectionType::CodexProgress => {
                    parts.push(format!("**Background progress:** {}\n", inj.content));
                }
                InjectionType::CodexError => {
                    parts.push(format!(
                        "**Background task failed:**\n{}\n",
                        inj.content
                    ));
                }
            }
        }

        parts.push("\nYou may acknowledge these updates naturally in conversation when relevant.".to_string());

        Some(parts.join("\n"))
    }

    /// Get recent injections (including acknowledged) for context
    pub async fn get_recent_injections(
        &self,
        voice_session_id: &str,
        limit: i32,
    ) -> Result<Vec<SessionInjection>> {
        let rows: Vec<(
            i64,
            String,
            String,
            String,
            String,
            Option<String>,
            i64,
            i64,
            Option<i64>,
            i32,
        )> = sqlx::query_as(
            r#"
            SELECT id, target_session_id, source_session_id, injection_type,
                   content, metadata, injected_at, acknowledged, acknowledged_at, sequence_num
            FROM session_injections
            WHERE target_session_id = ?
            ORDER BY injected_at DESC
            LIMIT ?
            "#,
        )
        .bind(voice_session_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        let injections = rows
            .into_iter()
            .map(|(id, target, source, inj_type, content, metadata, injected_at, ack, ack_at, seq)| {
                SessionInjection {
                    id,
                    target_session_id: target,
                    source_session_id: source,
                    injection_type: InjectionType::from_str(&inj_type)
                        .unwrap_or(InjectionType::CodexCompletion),
                    content,
                    metadata: metadata.and_then(|m| serde_json::from_str(&m).ok()),
                    injected_at,
                    acknowledged: ack != 0,
                    acknowledged_at: ack_at,
                    sequence_num: seq,
                }
            })
            .collect();

        Ok(injections)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn test_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .unwrap();

        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .unwrap();

        pool
    }

    #[tokio::test]
    async fn test_inject_and_retrieve_completion() {
        let pool = test_pool().await;
        let service = InjectionService::new(pool.clone());

        // Create a test session first
        sqlx::query(
            "INSERT INTO chat_sessions (id, session_type, message_count, created_at, last_active) VALUES ('voice-1', 'voice', 0, 0, 0)",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO chat_sessions (id, session_type, parent_session_id, message_count, created_at, last_active) VALUES ('codex-1', 'codex', 'voice-1', 0, 0, 0)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let metadata = CodexCompletionMetadata {
            files_changed: vec!["src/lib.rs".to_string(), "src/main.rs".to_string()],
            duration_seconds: 120,
            tokens_total: 50000,
            cost_usd: 0.50,
            tool_calls_count: 15,
            compaction_count: 2,
            key_actions: vec!["Implemented feature X".to_string()],
        };

        let injection_id = service
            .inject_codex_completion(
                "voice-1",
                "codex-1",
                "Successfully implemented feature X with full test coverage.",
                metadata,
            )
            .await
            .unwrap();

        assert!(injection_id > 0);

        // Retrieve pending
        let pending = service.get_pending_injections("voice-1").await.unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].injection_type, InjectionType::CodexCompletion);
        assert!(!pending[0].acknowledged);

        // Acknowledge
        service.acknowledge_injection(injection_id).await.unwrap();

        // Should be empty now
        let pending = service.get_pending_injections("voice-1").await.unwrap();
        assert!(pending.is_empty());
    }

    #[tokio::test]
    async fn test_format_for_prompt() {
        let pool = test_pool().await;
        let service = InjectionService::new(pool);

        let injections = vec![
            SessionInjection {
                id: 1,
                target_session_id: "voice-1".to_string(),
                source_session_id: "codex-1".to_string(),
                injection_type: InjectionType::CodexCompletion,
                content: "Completed implementing the feature.".to_string(),
                metadata: Some(serde_json::json!({
                    "files_changed": ["src/lib.rs", "src/main.rs"]
                })),
                injected_at: 0,
                acknowledged: false,
                acknowledged_at: None,
                sequence_num: 1,
            },
        ];

        let formatted = service.format_for_prompt(&injections);
        assert!(formatted.is_some());
        let text = formatted.unwrap();
        assert!(text.contains("Background Work Updates"));
        assert!(text.contains("Completed implementing the feature"));
        assert!(text.contains("src/lib.rs"));
    }
}
