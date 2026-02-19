// crates/mira-server/src/ipc/client/memory_ops.rs
//! HookClient methods for memory recall, behavior logging, observations,
//! compaction context, and auto-memory export.

use super::Backend;
use serde_json::json;

impl super::HookClient {
    /// Recall relevant memories for a project and query string.
    ///
    /// Accepts a `RecallContext` for passing user identity and branch info
    /// through to the semantic recall layer for better result ranking.
    pub async fn recall_memories(
        &mut self,
        ctx: &crate::hooks::recall::RecallContext,
        query: &str,
    ) -> Vec<String> {
        if self.is_ipc() {
            let mut params = json!({
                "project_id": ctx.project_id,
                "query": query,
            });
            if let Some(ref uid) = ctx.user_id {
                params["user_id"] = json!(uid);
            }
            if let Some(ref branch) = ctx.current_branch {
                params["current_branch"] = json!(branch);
            }
            if let Ok(result) = self.call("recall_memories", params).await {
                return result
                    .get("memories")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
            }
        }
        if let Backend::Direct { pool } = &self.inner {
            return crate::hooks::recall::recall_memories(pool, ctx, query).await;
        }
        Vec::new()
    }

    /// Log a behavior event. Fire-and-forget — errors are logged but not propagated.
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
                let mut tracker = crate::proactive::behavior::BehaviorTracker::for_session(
                    conn, session_id, project_id,
                );
                let et = match event_type.as_str() {
                    "tool_failure" => crate::proactive::EventType::ToolFailure,
                    "goal_update" => crate::proactive::EventType::GoalUpdate,
                    _ => crate::proactive::EventType::ToolUse,
                };
                if let Err(e) = tracker.log_event(conn, et, event_data) {
                    tracing::debug!("Failed to log behavior: {e}");
                }
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

    /// Export memories to MEMORY.mira.md. Fire-and-forget.
    pub async fn write_auto_memory(&mut self, project_id: i64, project_path: &str) {
        if self.is_ipc() {
            let params = json!({"project_id": project_id, "project_path": project_path});
            if let Ok(v) = self.call("write_auto_memory", params).await {
                let count = v.get("count").and_then(|c| c.as_i64()).unwrap_or(0);
                if count > 0 {
                    tracing::debug!("[mira] Auto-exported {} memories to MEMORY.mira.md", count);
                }
                return;
            }
            // fall through to Direct
        }
        if let Backend::Direct { pool } = &self.inner {
            let pool = pool.clone();
            let project_path = project_path.to_string();
            pool.try_interact_warn("auto memory export", move |conn| {
                if crate::tools::core::claude_local::auto_memory_dir_exists(&project_path) {
                    match crate::tools::core::claude_local::write_auto_memory_sync(
                        conn,
                        project_id,
                        &project_path,
                    ) {
                        Ok(count) if count > 0 => {
                            tracing::debug!(
                                "[mira] Auto-exported {} memories to MEMORY.mira.md",
                                count
                            );
                        }
                        Err(e) => {
                            tracing::warn!("[mira] Auto memory export failed: {}", e);
                        }
                        _ => {}
                    }
                }
                Ok(())
            })
            .await;
        }
    }
}
