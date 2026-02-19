// crates/mira-server/src/ipc/client/goal_ops.rs
//! HookClient methods for goals, milestones, error patterns, and prompt context.

use super::{Backend, UserPromptContextResult};
use serde_json::json;

impl super::HookClient {
    /// Get formatted active goals for a project.
    pub async fn get_active_goals(&mut self, project_id: i64, limit: usize) -> Vec<String> {
        if self.is_ipc() {
            let params = json!({"project_id": project_id, "limit": limit});
            if let Ok(result) = self.call("get_active_goals", params).await {
                return result
                    .get("goals")
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
            return crate::hooks::format_active_goals(pool, project_id, limit).await;
        }
        Vec::new()
    }

    /// Auto-link a completed task to goal milestones. Fire-and-forget.
    pub async fn auto_link_milestone(
        &mut self,
        project_id: i64,
        task_subject: &str,
        task_description: Option<&str>,
        session_id: Option<&str>,
    ) {
        if self.is_ipc() {
            let params = json!({
                "project_id": project_id,
                "task_subject": task_subject,
                "task_description": task_description,
                "session_id": session_id,
            });
            if self.call("auto_link_milestone", params).await.is_ok() {
                return;
            }
        }
        if let Backend::Direct { pool } = &self.inner {
            let pool = pool.clone();
            let task_subject = task_subject.to_string();
            let task_description = task_description.map(String::from);
            let session_id = session_id.map(String::from);
            pool.try_interact("auto_link_milestone", move |conn| {
                crate::hooks::task_completed::auto_link_milestone(
                    conn,
                    project_id,
                    &task_subject,
                    task_description.as_deref(),
                    session_id.as_deref(),
                )
            })
            .await;
        }
    }

    /// Store or update an error pattern. Fire-and-forget.
    pub async fn store_error_pattern(
        &mut self,
        project_id: i64,
        tool_name: &str,
        fingerprint: &str,
        template: &str,
        sample: &str,
        session_id: &str,
    ) {
        if self.is_ipc() {
            let params = json!({
                "project_id": project_id,
                "tool_name": tool_name,
                "fingerprint": fingerprint,
                "template": template,
                "sample": sample,
                "session_id": session_id,
            });
            if self.call("store_error_pattern", params).await.is_ok() {
                return;
            }
        }
        if let Backend::Direct { pool } = &self.inner {
            let pool = pool.clone();
            let tool_name = tool_name.to_string();
            let fingerprint = fingerprint.to_string();
            let template = template.to_string();
            let sample = sample.to_string();
            let session_id = session_id.to_string();
            pool.try_interact("store_error_pattern", move |conn| {
                crate::db::store_error_pattern_sync(
                    conn,
                    crate::db::StoreErrorPatternParams {
                        project_id,
                        tool_name: &tool_name,
                        error_fingerprint: &fingerprint,
                        error_template: &template,
                        raw_error_sample: &sample,
                        session_id: &session_id,
                    },
                )?;
                Ok(())
            })
            .await;
        }
    }

    /// Look up a resolved error pattern. Returns the fix description if found.
    pub async fn lookup_resolved_pattern(
        &mut self,
        project_id: i64,
        tool_name: &str,
        fingerprint: &str,
    ) -> Option<String> {
        if self.is_ipc() {
            let params = json!({
                "project_id": project_id,
                "tool_name": tool_name,
                "fingerprint": fingerprint,
            });
            if let Ok(result) = self.call("lookup_resolved_pattern", params).await {
                if result
                    .get("found")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    return result
                        .get("fix_description")
                        .and_then(|v| v.as_str())
                        .map(String::from);
                }
                return None;
            }
        }
        if let Backend::Direct { pool } = &self.inner {
            let pool = pool.clone();
            let tool_name = tool_name.to_string();
            let fingerprint = fingerprint.to_string();
            let pattern = pool
                .interact(move |conn| {
                    Ok::<_, anyhow::Error>(crate::db::lookup_resolved_pattern_sync(
                        conn,
                        project_id,
                        &tool_name,
                        &fingerprint,
                    ))
                })
                .await
                .ok()
                .flatten()?;
            return Some(pattern.fix_description);
        }
        None
    }

    /// Count how many times a tool has failed in the current session.
    pub async fn count_session_failures(&mut self, session_id: &str, tool_name: &str) -> i64 {
        if self.is_ipc() {
            let params = json!({"session_id": session_id, "tool_name": tool_name});
            if let Ok(result) = self.call("count_session_failures", params).await {
                return result.get("count").and_then(|v| v.as_i64()).unwrap_or(0);
            }
        }
        if let Backend::Direct { pool } = &self.inner {
            let pool = pool.clone();
            let session_id = session_id.to_string();
            let tool_name = tool_name.to_string();
            return pool
                .interact(move |conn| {
                    let count = conn
                        .query_row(
                            "SELECT COUNT(*) FROM session_behavior_log
                             WHERE session_id = ? AND event_type = 'tool_failure'
                               AND json_extract(event_data, '$.tool_name') = ?",
                            rusqlite::params![session_id, tool_name],
                            |row| row.get::<_, i64>(0),
                        )
                        .unwrap_or(0);
                    Ok::<_, anyhow::Error>(count)
                })
                .await
                .unwrap_or(0);
        }
        0
    }

    /// Resolve error patterns after a successful tool use.
    /// Returns true if a pattern was resolved.
    pub async fn resolve_error_patterns(
        &mut self,
        project_id: i64,
        session_id: &str,
        tool_name: &str,
    ) -> bool {
        if self.is_ipc() {
            let params = json!({
                "project_id": project_id,
                "session_id": session_id,
                "tool_name": tool_name,
            });
            if let Ok(result) = self.call("resolve_error_patterns", params).await {
                return result
                    .get("resolved")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
            }
        }
        if let Backend::Direct { pool } = &self.inner {
            let pool = pool.clone();
            let session_id = session_id.to_string();
            let tool_name = tool_name.to_string();
            return pool
                .interact(move |conn| {
                    let candidates = crate::db::get_unresolved_patterns_for_tool_sync(
                        conn,
                        project_id,
                        &tool_name,
                        &session_id,
                    );

                    let mut best: Option<(String, i64, i64)> = None;
                    for (_id, fingerprint) in &candidates {
                        let row: Option<(i64, i64)> = conn
                            .query_row(
                                "SELECT COUNT(*), COALESCE(MAX(sequence_position), 0)
                                 FROM session_behavior_log
                                 WHERE session_id = ? AND project_id = ?
                                   AND event_type = 'tool_failure'
                                   AND json_extract(event_data, '$.error_fingerprint') = ?",
                                rusqlite::params![&session_id, project_id, fingerprint],
                                |row| Ok((row.get(0)?, row.get(1)?)),
                            )
                            .ok();

                        if let Some((count, max_seq)) = row
                            && count >= 3
                        {
                            let dominated = match &best {
                                None => true,
                                Some((_, _, best_seq)) => max_seq > *best_seq,
                            };
                            if dominated {
                                best = Some((fingerprint.clone(), count, max_seq));
                            }
                        }
                    }

                    if let Some((fingerprint, session_fp_count, _)) = best {
                        let _ = crate::db::resolve_error_pattern_sync(
                            conn,
                            project_id,
                            &tool_name,
                            &fingerprint,
                            &session_id,
                            &format!(
                                "Tool '{}' succeeded after {} session failures of this pattern",
                                tool_name, session_fp_count
                            ),
                        );
                        return Ok::<_, anyhow::Error>(true);
                    }
                    Ok(false)
                })
                .await
                .unwrap_or(false);
        }
        false
    }

    /// Get all context needed by the UserPromptSubmit hook in a single call.
    /// Only available via IPC (returns None in Direct mode — caller handles fallback).
    pub async fn get_user_prompt_context(
        &mut self,
        message: &str,
        session_id: &str,
    ) -> Option<UserPromptContextResult> {
        if !self.is_ipc() {
            return None;
        }
        let params = json!({"message": message, "session_id": session_id});
        let v = self.call("get_user_prompt_context", params).await.ok()?;

        Some(UserPromptContextResult {
            project_id: v.get("project_id").and_then(|v| v.as_i64()),
            project_path: v
                .get("project_path")
                .and_then(|v| v.as_str())
                .map(String::from),
            reactive_context: v
                .get("reactive_context")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            reactive_sources: v
                .get("reactive_sources")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|s| s.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
            reactive_from_cache: v
                .get("reactive_from_cache")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            reactive_summary: v
                .get("reactive_summary")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            reactive_skip_reason: v
                .get("reactive_skip_reason")
                .and_then(|v| v.as_str())
                .map(String::from),
            proactive_context: v
                .get("proactive_context")
                .and_then(|v| v.as_str())
                .map(String::from),
            team_context: v
                .get("team_context")
                .and_then(|v| v.as_str())
                .map(String::from),
            cross_project_context: v
                .get("cross_project_context")
                .and_then(|v| v.as_str())
                .map(String::from),
            config_max_chars: v
                .get("config_max_chars")
                .and_then(|v| v.as_u64())
                .unwrap_or(3000) as usize,
        })
    }
}
