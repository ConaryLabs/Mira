// crates/mira-server/src/ipc/client/state_ops.rs
//! HookClient methods for behavior logging, observations, and compaction context.

use super::Backend;
use serde_json::json;

impl super::HookClient {
    /// Log a behavior event. Fire-and-forget -- errors are logged but not propagated.
    pub async fn log_behavior(
        &mut self,
        session_id: &str,
        project_id: i64,
        event_type: &str,
        event_data: serde_json::Value,
    ) {
        if session_id.is_empty() {
            return;
        }
        if self.is_ipc() {
            let params = json!({
                "session_id": session_id,
                "project_id": project_id,
                "event_type": event_type,
                "event_data": event_data,
            });
            if self.call("log_behavior", params).await.is_ok() {
                return;
            }
        }
        if let Backend::Direct { pool } = &self.inner {
            let pool = pool.clone();
            let session_id = session_id.to_string();
            let event_type = event_type.to_string();
            pool.try_interact("log_behavior", move |conn| {
                let event_data_str =
                    serde_json::to_string(&event_data).unwrap_or_else(|_| "{}".to_string());
                let seq: i64 = conn
                    .query_row(
                        "SELECT COALESCE(MAX(sequence_position), 0) + 1 FROM session_behavior_log WHERE session_id = ?",
                        rusqlite::params![&session_id],
                        |row| row.get(0),
                    )
                    .unwrap_or(1);
                conn.execute(
                    "INSERT INTO session_behavior_log (session_id, project_id, event_type, event_data, sequence_position)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    rusqlite::params![&session_id, project_id, &event_type, &event_data_str, seq],
                ).ok();
                Ok(())
            })
            .await;
        }
    }

    /// Store an observation. Fire-and-forget.
    #[allow(clippy::too_many_arguments)]
    pub async fn store_observation(
        &mut self,
        project_id: Option<i64>,
        content: &str,
        observation_type: &str,
        category: Option<&str>,
        confidence: f64,
        source: &str,
        scope: &str,
        expires_at: Option<&str>,
    ) {
        if self.is_ipc() {
            let params = json!({
                "project_id": project_id,
                "content": content,
                "observation_type": observation_type,
                "category": category,
                "confidence": confidence,
                "source": source,
                "scope": scope,
                "expires_at": expires_at,
            });
            if self.call("store_observation", params).await.is_ok() {
                return;
            }
        }
        if let Backend::Direct { pool } = &self.inner {
            let pool = pool.clone();
            let content = content.to_string();
            let observation_type = observation_type.to_string();
            let category = category.map(String::from);
            let source = source.to_string();
            let scope = scope.to_string();
            let expires_at = expires_at.map(String::from);
            pool.try_interact("store_observation", move |conn| {
                crate::db::store_observation_sync(
                    conn,
                    crate::db::StoreObservationParams {
                        project_id,
                        key: None,
                        content: &content,
                        observation_type: &observation_type,
                        category: category.as_deref(),
                        confidence,
                        source: &source,
                        session_id: None,
                        team_id: None,
                        scope: &scope,
                        expires_at: expires_at.as_deref(),
                    },
                )?;
                Ok(())
            })
            .await;
        }
    }

    /// Generate a code context bundle for a scope. Returns the bundle content string,
    /// or None if IPC is unavailable, the index is empty, or the query fails.
    ///
    /// IPC-only: code_pool is only available on the server, not in direct DB fallback.
    pub async fn generate_bundle(
        &mut self,
        project_id: i64,
        scope: &str,
        budget: i64,
        depth: &str,
    ) -> Option<String> {
        if !self.is_ipc() {
            return None;
        }
        let params = json!({
            "project_id": project_id,
            "scope": scope,
            "budget": budget,
            "depth": depth,
        });
        let result = self.call("generate_bundle", params).await.ok()?;
        if result
            .get("empty")
            .and_then(|v| v.as_bool())
            .unwrap_or(true)
        {
            return None;
        }
        result
            .get("content")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from)
    }

    /// Save compaction context to session_snapshots. Fire-and-forget.
    pub async fn save_compaction_context(&mut self, session_id: &str, context: serde_json::Value) {
        if self.is_ipc() {
            let params = json!({"session_id": session_id, "context": context});
            if self.call("save_compaction_context", params).await.is_ok() {
                return;
            }
        }
        if let Backend::Direct { pool } = &self.inner {
            let pool = pool.clone();
            let session_id = session_id.to_string();
            pool.try_interact("save_compaction_context", move |conn| {
                // Use BEGIN IMMEDIATE for atomic read-modify-write (matches IPC path in ops.rs)
                conn.execute_batch("BEGIN IMMEDIATE")
                    .map_err(|e| anyhow::anyhow!("{e}"))?;

                let result: Result<(), anyhow::Error> = (|| {
                    let existing: Option<String> = conn
                        .query_row(
                            "SELECT snapshot FROM session_snapshots WHERE session_id = ?",
                            [&session_id],
                            |row| row.get::<_, String>(0),
                        )
                        .ok();

                    let mut snapshot = if let Some(ref json_str) = existing {
                        serde_json::from_str::<serde_json::Value>(json_str)
                            .unwrap_or_else(|_| json!({}))
                    } else {
                        json!({})
                    };

                    snapshot["compaction_context"] =
                        if let Some(existing) = snapshot.get("compaction_context") {
                            crate::hooks::precompact::merge_compaction_contexts(existing, &context)
                        } else {
                            context
                        };

                    let snapshot_str =
                        serde_json::to_string(&snapshot).map_err(|e| anyhow::anyhow!("{e}"))?;

                    conn.execute(
                        "INSERT INTO session_snapshots (session_id, snapshot, created_at)
                         VALUES (?1, ?2, datetime('now'))
                         ON CONFLICT(session_id) DO UPDATE SET snapshot = ?2, created_at = datetime('now')",
                        rusqlite::params![session_id, snapshot_str],
                    )
                    .map_err(|e| anyhow::anyhow!("{e}"))?;

                    Ok(())
                })();

                match result {
                    Ok(()) => match conn.execute_batch("COMMIT") {
                        Ok(()) => Ok(()),
                        Err(commit_err) => {
                            // COMMIT failed -- rollback to clean up the open transaction
                            if let Err(rb_err) = conn.execute_batch("ROLLBACK") {
                                tracing::warn!(error = %rb_err, "ROLLBACK failed after COMMIT failure");
                            }
                            Err(anyhow::anyhow!("COMMIT failed: {commit_err}"))
                        }
                    },
                    Err(e) => {
                        // Best-effort rollback -- log but don't mask the original error
                        if let Err(rb_err) = conn.execute_batch("ROLLBACK") {
                            tracing::warn!(error = %rb_err, "ROLLBACK failed after save_compaction_context error");
                        }
                        Err(e)
                    }
                }
            })
            .await;
        }
    }
}
