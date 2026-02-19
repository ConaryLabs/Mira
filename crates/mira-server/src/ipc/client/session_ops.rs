// crates/mira-server/src/ipc/client/session_ops.rs
//! HookClient methods for session lifecycle: registration, context building,
//! closing, task snapshots, and CLAUDE.local.md export.

use super::Backend;
use serde_json::json;

impl super::HookClient {
    /// Register a session in the database. Returns project_id.
    pub async fn register_session(
        &mut self,
        session_id: &str,
        cwd: &str,
        source: &str,
    ) -> Option<i64> {
        if self.is_ipc() {
            let params = json!({
                "session_id": session_id,
                "cwd": cwd,
                "source": source,
            });
            if let Ok(result) = self.call("register_session", params).await {
                return result.get("project_id")?.as_i64();
            }
        }
        if let Backend::Direct { pool } = &self.inner {
            let pool = pool.clone();
            let session_id = session_id.to_string();
            let cwd = cwd.to_string();
            let source = source.to_string();
            return pool
                .run(move |conn| {
                    let (project_id, _) = crate::db::get_or_create_project_sync(conn, &cwd, None)?;
                    crate::db::create_session_ext_sync(
                        conn,
                        &session_id,
                        Some(project_id),
                        Some(&source),
                        None,
                    )?;
                    conn.execute(
                        "INSERT INTO session_behavior_log (session_id, event_type, event_data) \
                         VALUES (?1, 'session_start', ?2)",
                        rusqlite::params![session_id, source],
                    )
                    .ok();
                    Ok::<_, rusqlite::Error>(project_id)
                })
                .await
                .ok();
        }
        None
    }

    /// Register a team session. Returns team_id.
    #[allow(clippy::too_many_arguments)]
    pub async fn register_team_session(
        &mut self,
        team_name: &str,
        config_path: &str,
        member_name: &str,
        role: &str,
        agent_type: Option<&str>,
        session_id: &str,
        cwd: Option<&str>,
    ) -> Option<i64> {
        if self.is_ipc() {
            let params = json!({
                "team_name": team_name,
                "config_path": config_path,
                "member_name": member_name,
                "role": role,
                "agent_type": agent_type,
                "session_id": session_id,
                "cwd": cwd,
            });
            if let Ok(result) = self.call("register_team_session", params).await {
                return result.get("team_id")?.as_i64();
            }
        }
        if let Backend::Direct { pool } = &self.inner {
            let pool = pool.clone();
            let team_name = team_name.to_string();
            let config_path = config_path.to_string();
            let member_name = member_name.to_string();
            let role = role.to_string();
            let agent_type = agent_type.map(String::from);
            let session_id = session_id.to_string();
            let cwd = cwd.map(String::from);
            return pool
                .interact(move |conn| {
                    let project_id: Option<i64> = if let Some(ref cwd_path) = cwd {
                        crate::db::get_or_create_project_sync(conn, cwd_path, None)
                            .ok()
                            .map(|(id, _)| id)
                    } else {
                        None
                    };
                    let tid = crate::db::get_or_create_team_sync(
                        conn,
                        &team_name,
                        project_id,
                        &config_path,
                    )?;
                    crate::db::register_team_session_sync(
                        conn,
                        tid,
                        &session_id,
                        &member_name,
                        &role,
                        agent_type.as_deref(),
                    )?;
                    Ok::<_, anyhow::Error>(tid)
                })
                .await
                .ok();
        }
        None
    }

    /// Get startup context for a fresh session.
    pub async fn get_startup_context(
        &mut self,
        cwd: Option<&str>,
        session_id: Option<&str>,
    ) -> Option<String> {
        if self.is_ipc() {
            let mut params = json!({"cwd": cwd});
            if let Some(s) = session_id {
                params["session_id"] = json!(s);
            }
            if let Ok(result) = self.call("get_startup_context", params).await {
                return result.get("context")?.as_str().map(String::from);
            }
        }
        if let Backend::Direct { pool } = &self.inner {
            return crate::hooks::session::build_startup_context(
                cwd,
                Some(pool.clone()),
                session_id,
            )
            .await;
        }
        None
    }

    /// Get resume context for a resumed session.
    pub async fn get_resume_context(
        &mut self,
        cwd: Option<&str>,
        session_id: Option<&str>,
    ) -> Option<String> {
        if self.is_ipc() {
            let params = json!({"cwd": cwd, "session_id": session_id});
            if let Ok(result) = self.call("get_resume_context", params).await {
                return result.get("context")?.as_str().map(String::from);
            }
        }
        if let Backend::Direct { pool } = &self.inner {
            return crate::hooks::session::build_resume_context(
                cwd,
                session_id,
                Some(pool.clone()),
            )
            .await;
        }
        None
    }

    /// Close a session: build summary, save snapshot, update status. Fire-and-forget.
    pub async fn close_session(&mut self, session_id: &str) {
        if self.is_ipc() {
            let params = json!({"session_id": session_id});
            if self.call("close_session", params).await.is_ok() {
                return;
            }
        }
        if let Backend::Direct { pool } = &self.inner {
            let pool = pool.clone();
            let session_id = session_id.to_string();
            pool.try_interact_warn("session close", move |conn| {
                if let Err(e) = crate::db::set_server_state_sync(
                    conn,
                    "last_stop_time",
                    &chrono::Utc::now().to_rfc3339(),
                ) {
                    tracing::warn!("[mira] Failed to save server state: {e}");
                }
                let summary = if !session_id.is_empty() {
                    crate::hooks::stop::build_session_summary(conn, &session_id)
                } else {
                    None
                };
                if !session_id.is_empty() {
                    if let Err(e) = crate::hooks::stop::save_session_snapshot(conn, &session_id) {
                        tracing::warn!("[mira] Session snapshot failed: {}", e);
                    }
                    if let Err(e) =
                        crate::db::close_session_sync(conn, &session_id, summary.as_deref())
                    {
                        tracing::warn!("[mira] Failed to close session: {e}");
                    }
                    tracing::debug!(
                        "[mira] Closed session {}",
                        crate::utils::truncate_at_boundary(&session_id, 8)
                    );
                }
                Ok(())
            })
            .await;
        }
    }

    /// Snapshot native Claude Code tasks. Fire-and-forget.
    pub async fn snapshot_tasks(
        &mut self,
        project_id: i64,
        list_id: &str,
        session_id: Option<&str>,
        tasks: &[crate::tasks::NativeTask],
        is_session_end: bool,
    ) {
        let (completed, remaining) = tasks.iter().fold((0usize, 0usize), |(c, r), t| {
            if t.status == "completed" {
                (c + 1, r)
            } else {
                (c, r + 1)
            }
        });

        let mut ipc_ok = false;
        if self.is_ipc() {
            let tasks_json = match serde_json::to_value(tasks) {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!("[mira] Failed to serialize tasks: {}", e);
                    return;
                }
            };
            let params = json!({
                "project_id": project_id,
                "list_id": list_id,
                "session_id": session_id,
                "tasks": tasks_json,
            });
            if let Ok(v) = self.call("snapshot_tasks", params).await {
                let count = v.get("count").and_then(|c| c.as_u64()).unwrap_or(0) as usize;
                let label = if is_session_end { "SessionEnd" } else { "Stop" };
                tracing::debug!(
                    "[mira] {} snapshot: {} tasks ({} completed, {} remaining)",
                    label,
                    count,
                    completed,
                    remaining,
                );
                ipc_ok = true;
            }
            // fall through to Direct
        }
        if !ipc_ok && let Backend::Direct { pool } = &self.inner {
            let pool = pool.clone();
            let list_id = list_id.to_string();
            let session_id = session_id.map(String::from);
            let tasks = tasks.to_vec();
            match pool
                .interact(move |conn| {
                    let count = crate::db::session_tasks::snapshot_native_tasks_sync(
                        conn,
                        project_id,
                        &list_id,
                        session_id.as_deref(),
                        &tasks,
                    )?;
                    Ok::<_, anyhow::Error>(count)
                })
                .await
            {
                Ok(count) => {
                    let label = if is_session_end { "SessionEnd" } else { "Stop" };
                    tracing::debug!(
                        "[mira] {} snapshot: {} tasks ({} completed, {} remaining)",
                        label,
                        count,
                        completed,
                        remaining,
                    );
                }
                Err(e) => {
                    tracing::warn!("[mira] Task snapshot failed: {}", e);
                }
            }
        }
    }

    /// Auto-export memories to CLAUDE.local.md. Fire-and-forget.
    pub async fn write_claude_local_md(&mut self, project_id: i64) {
        if self.is_ipc() {
            let params = json!({"project_id": project_id});
            if let Ok(v) = self.call("write_claude_local_md", params).await {
                let count = v.get("count").and_then(|c| c.as_i64()).unwrap_or(0);
                if count > 0 {
                    tracing::debug!("[mira] Auto-exported {} memories to CLAUDE.local.md", count);
                }
                return;
            }
            // fall through to Direct
        }
        if let Backend::Direct { pool } = &self.inner {
            let pool = pool.clone();
            pool.try_interact_warn("CLAUDE.local.md export", move |conn| {
                let path = crate::db::get_last_active_project_sync(conn).unwrap_or_else(|e| {
                    tracing::warn!("Failed to get last active project: {e}");
                    None
                });
                if let Some(project_path) = path {
                    match crate::tools::core::claude_local::write_claude_local_md_sync(
                        conn,
                        project_id,
                        &project_path,
                    ) {
                        Ok(count) if count > 0 => {
                            tracing::debug!(
                                "[mira] Auto-exported {} memories to CLAUDE.local.md",
                                count
                            );
                        }
                        Err(e) => {
                            tracing::warn!("[mira] CLAUDE.local.md export failed: {}", e);
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
