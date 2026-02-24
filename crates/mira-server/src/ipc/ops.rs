// crates/mira-server/src/ipc/ops.rs
// IPC operation implementations

use crate::mcp::MiraServer;
use anyhow::Result;
use serde_json::{Value, json};

/// Resolve a project by CWD, per-session state, or last active project.
pub async fn resolve_project(server: &MiraServer, params: Value) -> Result<Value> {
    let cwd = params.get("cwd").and_then(|v| v.as_str());
    let session_id = params
        .get("session_id")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());

    if let Some(cwd_val) = cwd {
        // Explicit cwd provided — use it directly
        let cwd_owned = cwd_val.to_string();
        let (id, path) = server
            .pool
            .interact(move |conn| {
                let (id, _name) = crate::db::get_or_create_project_sync(conn, &cwd_owned, None)
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                Ok((id, cwd_owned))
            })
            .await?;
        Ok(json!({"project_id": id, "path": path}))
    } else {
        // No explicit cwd — use per-session/global file resolution
        let (id, path, _name) = crate::hooks::resolve_project(&server.pool, session_id).await;
        match (id, path) {
            (Some(id), Some(path)) => Ok(json!({"project_id": id, "path": path})),
            _ => anyhow::bail!(
                "no cwd provided and no active project found (checked per-session file, global file, and database)"
            ),
        }
    }
}

/// Recall memories relevant to a query for the given project.
pub async fn recall_memories(server: &MiraServer, params: Value) -> Result<Value> {
    let project_id = params
        .get("project_id")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| anyhow::anyhow!("missing required param: project_id"))?;

    let query = params
        .get("query")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing required param: query"))?;

    let user_id = params
        .get("user_id")
        .and_then(|v| v.as_str())
        .map(String::from);
    let current_branch = params
        .get("current_branch")
        .and_then(|v| v.as_str())
        .map(String::from);

    let ctx = crate::hooks::recall::RecallContext {
        project_id,
        user_id,
        current_branch,
    };
    let memories = crate::hooks::recall::recall_memories(&server.pool, &ctx, query).await;
    Ok(json!({"memories": memories}))
}

/// Log a behavior event to session_behavior_log.
pub async fn log_behavior(server: &MiraServer, params: Value) -> Result<Value> {
    let session_id = params
        .get("session_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // Nothing to log without a session
    if session_id.is_empty() {
        return Ok(json!({}));
    }

    let project_id = params
        .get("project_id")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| anyhow::anyhow!("missing required param: project_id"))?;
    let event_type = params
        .get("event_type")
        .and_then(|v| v.as_str())
        .unwrap_or("tool_use")
        .to_string();
    let event_data = params.get("event_data").cloned().unwrap_or(json!({}));

    server
        .pool
        .interact(move |conn| {
            let mut tracker = crate::proactive::behavior::BehaviorTracker::for_session(
                conn, session_id, project_id,
            );
            let et = match event_type.as_str() {
                "tool_failure" => crate::proactive::EventType::ToolFailure,
                "goal_update" => crate::proactive::EventType::GoalUpdate,
                _ => crate::proactive::EventType::ToolUse,
            };
            tracker
                .log_event(conn, et, event_data)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            Ok::<_, anyhow::Error>(())
        })
        .await?;

    Ok(json!({}))
}

/// Store an observation (used by precompact, subagent_stop).
pub async fn store_observation(server: &MiraServer, params: Value) -> Result<Value> {
    let project_id = params.get("project_id").and_then(|v| v.as_i64());
    let content = params
        .get("content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing required param: content"))?
        .to_string();
    let observation_type = params
        .get("observation_type")
        .and_then(|v| v.as_str())
        .unwrap_or("general")
        .to_string();
    let category = params
        .get("category")
        .and_then(|v| v.as_str())
        .map(String::from);
    let confidence = params
        .get("confidence")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.5);
    let source = params
        .get("source")
        .and_then(|v| v.as_str())
        .unwrap_or("hook")
        .to_string();
    let scope = params
        .get("scope")
        .and_then(|v| v.as_str())
        .unwrap_or("project")
        .to_string();
    let expires_at = params
        .get("expires_at")
        .and_then(|v| v.as_str())
        .map(String::from);

    server
        .pool
        .interact(move |conn| {
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
            )
            .map_err(|e| anyhow::anyhow!("{e}"))
        })
        .await?;

    Ok(json!({}))
}

/// Get active goals for a project.
pub async fn get_active_goals(server: &MiraServer, params: Value) -> Result<Value> {
    let project_id = params
        .get("project_id")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| anyhow::anyhow!("missing required param: project_id"))?;
    let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(5) as usize;

    let goals = crate::hooks::format_active_goals(&server.pool, project_id, limit).await;
    Ok(json!({"goals": goals}))
}

/// Store or update an error pattern for cross-session learning.
pub async fn store_error_pattern(server: &MiraServer, params: Value) -> Result<Value> {
    let project_id = params
        .get("project_id")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| anyhow::anyhow!("missing required param: project_id"))?;
    let tool_name = params
        .get("tool_name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing required param: tool_name"))?
        .to_string();
    let fingerprint = params
        .get("fingerprint")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing required param: fingerprint"))?
        .to_string();
    let template = params
        .get("template")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing required param: template"))?
        .to_string();
    let sample = params
        .get("sample")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let session_id = params
        .get("session_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    server
        .pool
        .interact(move |conn| {
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
            Ok::<_, anyhow::Error>(())
        })
        .await?;

    Ok(json!({}))
}

/// Look up a resolved error pattern by fingerprint.
pub async fn lookup_resolved_pattern(server: &MiraServer, params: Value) -> Result<Value> {
    let project_id = params
        .get("project_id")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| anyhow::anyhow!("missing required param: project_id"))?;
    let tool_name = params
        .get("tool_name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing required param: tool_name"))?
        .to_string();
    let fingerprint = params
        .get("fingerprint")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing required param: fingerprint"))?
        .to_string();

    let pattern = server
        .pool
        .interact(move |conn| {
            Ok::<_, anyhow::Error>(crate::db::lookup_resolved_pattern_sync(
                conn,
                project_id,
                &tool_name,
                &fingerprint,
            ))
        })
        .await?;

    match pattern {
        Some(p) => Ok(json!({"found": true, "fix_description": p.fix_description})),
        None => Ok(json!({"found": false})),
    }
}

/// Count how many times a tool has failed in the current session.
pub async fn count_session_failures(server: &MiraServer, params: Value) -> Result<Value> {
    let session_id = params
        .get("session_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing required param: session_id"))?
        .to_string();
    let tool_name = params
        .get("tool_name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing required param: tool_name"))?
        .to_string();

    let count: i64 = server
        .pool
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
        .await?;

    Ok(json!({"count": count}))
}

/// Resolve error patterns after a successful tool use.
pub async fn resolve_error_patterns(server: &MiraServer, params: Value) -> Result<Value> {
    let project_id = params
        .get("project_id")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| anyhow::anyhow!("missing required param: project_id"))?;
    let session_id = params
        .get("session_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing required param: session_id"))?
        .to_string();
    let tool_name = params
        .get("tool_name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing required param: tool_name"))?
        .to_string();

    let resolved = server
        .pool
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
        .await?;

    Ok(json!({"resolved": resolved}))
}

/// Get team membership for a session.
pub async fn get_team_membership(server: &MiraServer, params: Value) -> Result<Value> {
    let session_id = params
        .get("session_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing required param: session_id"))?
        .to_string();

    let membership = server
        .pool
        .interact(move |conn| {
            Ok::<_, anyhow::Error>(crate::db::get_team_membership_for_session_sync(
                conn,
                &session_id,
            ))
        })
        .await?;

    match membership {
        Some(m) => Ok(json!({
            "found": true,
            "team_id": m.team_id,
            "team_name": m.team_name,
            "member_name": m.member_name,
            "role": m.role,
        })),
        None => Ok(json!({"found": false})),
    }
}

/// Record file ownership for team conflict detection.
pub async fn record_file_ownership(server: &MiraServer, params: Value) -> Result<Value> {
    let team_id = params
        .get("team_id")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| anyhow::anyhow!("missing required param: team_id"))?;
    let session_id = params
        .get("session_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing required param: session_id"))?
        .to_string();
    let member_name = params
        .get("member_name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing required param: member_name"))?
        .to_string();
    let file_path = params
        .get("file_path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing required param: file_path"))?
        .to_string();
    let tool_name = params
        .get("tool_name")
        .and_then(|v| v.as_str())
        .unwrap_or("Edit")
        .to_string();

    server
        .pool
        .interact(move |conn| {
            crate::db::record_file_ownership_sync(
                conn,
                team_id,
                &session_id,
                &member_name,
                &file_path,
                &tool_name,
            )
            .map_err(|e| anyhow::anyhow!("{e}"))
        })
        .await?;

    Ok(json!({}))
}

/// Get file conflicts for a team session.
pub async fn get_file_conflicts(server: &MiraServer, params: Value) -> Result<Value> {
    let team_id = params
        .get("team_id")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| anyhow::anyhow!("missing required param: team_id"))?;
    let session_id = params
        .get("session_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing required param: session_id"))?
        .to_string();

    let conflicts: Vec<crate::db::FileConflict> = server
        .pool
        .interact(move |conn| {
            Ok::<_, anyhow::Error>(crate::db::get_file_conflicts_sync(
                conn,
                team_id,
                &session_id,
            ))
        })
        .await?;

    let conflicts_json: Vec<Value> = conflicts
        .iter()
        .map(|c| {
            json!({
                "file_path": c.file_path,
                "other_member_name": c.other_member_name,
                "operation": c.operation,
            })
        })
        .collect();
    Ok(json!({"conflicts": conflicts_json}))
}

/// Auto-link a completed task to goal milestones.
pub async fn auto_link_milestone(server: &MiraServer, params: Value) -> Result<Value> {
    let project_id = params
        .get("project_id")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| anyhow::anyhow!("missing required param: project_id"))?;
    let task_subject = params
        .get("task_subject")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing required param: task_subject"))?
        .to_string();
    let task_description = params
        .get("task_description")
        .and_then(|v| v.as_str())
        .map(String::from);
    let session_id = params
        .get("session_id")
        .and_then(|v| v.as_str())
        .map(String::from);

    server
        .pool
        .interact(move |conn| {
            crate::hooks::task_completed::auto_link_milestone(
                conn,
                project_id,
                &task_subject,
                task_description.as_deref(),
                session_id.as_deref(),
            )
        })
        .await?;

    Ok(json!({}))
}

/// Save compaction context to session_snapshots.
pub async fn save_compaction_context(server: &MiraServer, params: Value) -> Result<Value> {
    let session_id = params
        .get("session_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing required param: session_id"))?
        .to_string();
    let context = params
        .get("context")
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("missing required param: context"))?;

    server
        .pool
        .interact(move |conn| {
            // Use an explicit transaction to make the read-modify-write atomic.
            // Without this, another hook could race between the SELECT and INSERT.
            conn.execute_batch("BEGIN IMMEDIATE;")
                .map_err(|e| anyhow::anyhow!("begin transaction: {e}"))?;

            let result = (|| -> Result<()> {
                let existing: Option<String> = conn
                    .query_row(
                        "SELECT snapshot FROM session_snapshots WHERE session_id = ?",
                        [&session_id],
                        |row| row.get::<_, String>(0),
                    )
                    .ok();

                let mut snapshot = if let Some(ref json_str) = existing {
                    serde_json::from_str::<Value>(json_str).unwrap_or_else(|_| json!({}))
                } else {
                    json!({})
                };

                snapshot["compaction_context"] = if let Some(existing) = snapshot.get("compaction_context") {
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
                Ok(()) => {
                    conn.execute_batch("COMMIT;")
                        .map_err(|e| anyhow::anyhow!("commit: {e}"))?;
                    Ok(())
                }
                Err(e) => {
                    let _ = conn.execute_batch("ROLLBACK;");
                    Err(e)
                }
            }
        })
        .await?;

    Ok(json!({}))
}

// ═══════════════════════════════════════════════════════════════════════════════
// Phase 3: Session lifecycle & stop operations
// ═══════════════════════════════════════════════════════════════════════════════

/// Register a new session (create project + session + log start event).
pub async fn register_session(server: &MiraServer, params: Value) -> Result<Value> {
    let session_id = params
        .get("session_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing required param: session_id"))?
        .to_string();
    let cwd = params
        .get("cwd")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing required param: cwd"))?
        .to_string();
    let source = params
        .get("source")
        .and_then(|v| v.as_str())
        .unwrap_or("startup")
        .to_string();

    let project_id = server
        .pool
        .interact(move |conn| {
            let (project_id, _) = crate::db::get_or_create_project_sync(conn, &cwd, None)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
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
            .ok(); // best-effort
            Ok::<_, anyhow::Error>(project_id)
        })
        .await?;

    Ok(json!({"project_id": project_id}))
}

/// Register a team session (create team + register member).
pub async fn register_team_session(server: &MiraServer, params: Value) -> Result<Value> {
    let team_name = params
        .get("team_name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing required param: team_name"))?
        .to_string();
    let config_path = params
        .get("config_path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing required param: config_path"))?
        .to_string();
    let member_name = params
        .get("member_name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing required param: member_name"))?
        .to_string();
    let role = params
        .get("role")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing required param: role"))?
        .to_string();
    let agent_type = params
        .get("agent_type")
        .and_then(|v| v.as_str())
        .map(String::from);
    let session_id = params
        .get("session_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing required param: session_id"))?
        .to_string();
    let cwd = params.get("cwd").and_then(|v| v.as_str()).map(String::from);

    let team_id = server
        .pool
        .interact(move |conn| {
            let project_id: Option<i64> = if let Some(ref cwd_path) = cwd {
                crate::db::get_or_create_project_sync(conn, cwd_path, None)
                    .ok()
                    .map(|(id, _)| id)
            } else {
                None
            };
            let tid =
                crate::db::get_or_create_team_sync(conn, &team_name, project_id, &config_path)?;
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
        .await?;

    Ok(json!({"team_id": team_id}))
}

/// Get startup context (for fresh sessions).
pub async fn get_startup_context(server: &MiraServer, params: Value) -> Result<Value> {
    let cwd = params.get("cwd").and_then(|v| v.as_str());
    let session_id = params
        .get("session_id")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());
    let context =
        crate::hooks::session::build_startup_context(cwd, Some(server.pool.clone()), session_id)
            .await;
    Ok(json!({"context": context}))
}

/// Get resume context (for resumed sessions).
pub async fn get_resume_context(server: &MiraServer, params: Value) -> Result<Value> {
    let cwd = params.get("cwd").and_then(|v| v.as_str());
    let session_id = params.get("session_id").and_then(|v| v.as_str());
    let context =
        crate::hooks::session::build_resume_context(cwd, session_id, Some(server.pool.clone()))
            .await;
    Ok(json!({"context": context}))
}

/// Close a session: build summary, save snapshot, update status.
///
/// This is a cleanup operation — individual steps are best-effort.
/// Each step logs its own error and continues, because partial cleanup
/// is better than aborting and leaving the session in a broken state.
pub async fn close_session(server: &MiraServer, params: Value) -> Result<Value> {
    let session_id = params
        .get("session_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing required param: session_id"))?
        .to_string();

    server
        .pool
        .interact(move |conn| {
            // Best-effort: record the stop timestamp regardless of session validity
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
                // Best-effort: snapshot may fail if session has no data yet
                if let Err(e) = crate::hooks::stop::save_session_snapshot(conn, &session_id) {
                    tracing::warn!("[mira] Session snapshot failed: {}", e);
                }
                // Best-effort: close may fail if session was already closed
                if let Err(e) = crate::db::close_session_sync(conn, &session_id, summary.as_deref())
                {
                    tracing::warn!("[mira] Failed to close session: {e}");
                }
                tracing::debug!(
                    "[mira] Closed session {}",
                    crate::utils::truncate_at_boundary(&session_id, 8)
                );
            }
            Ok::<_, anyhow::Error>(())
        })
        .await?;

    Ok(json!({}))
}

/// Snapshot native Claude Code tasks into the database.
pub async fn snapshot_tasks(server: &MiraServer, params: Value) -> Result<Value> {
    let project_id = params
        .get("project_id")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| anyhow::anyhow!("missing required param: project_id"))?;
    let list_id = params
        .get("list_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let session_id = params
        .get("session_id")
        .and_then(|v| v.as_str())
        .map(String::from);
    let tasks_json = params
        .get("tasks")
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("missing required param: tasks"))?;

    // Reject unreasonably large task arrays before deserializing
    if let Some(arr) = tasks_json.as_array()
        && arr.len() > 10_000
    {
        anyhow::bail!("too many tasks: {} (max 10000)", arr.len());
    }

    let tasks: Vec<crate::tasks::NativeTask> = serde_json::from_value(tasks_json)
        .map_err(|e| anyhow::anyhow!("failed to deserialize tasks: {e}"))?;

    let count = server
        .pool
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
        .await?;

    Ok(json!({"count": count}))
}

/// Auto-export ranked memories to CLAUDE.local.md.
pub async fn write_claude_local_md(server: &MiraServer, params: Value) -> Result<Value> {
    let project_id = params
        .get("project_id")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| anyhow::anyhow!("missing required param: project_id"))?;

    let count = server
        .pool
        .interact(move |conn| {
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
                    Ok(count) => Ok::<_, anyhow::Error>(count),
                    Err(e) => {
                        tracing::warn!("[mira] CLAUDE.local.md export failed: {}", e);
                        Ok(0)
                    }
                }
            } else {
                Ok(0)
            }
        })
        .await?;

    Ok(json!({"count": count}))
}

/// Deactivate a team session (set status='stopped').
pub async fn deactivate_team_session(server: &MiraServer, params: Value) -> Result<Value> {
    let session_id = params
        .get("session_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing required param: session_id"))?
        .to_string();

    server
        .pool
        .run(move |conn| crate::db::deactivate_team_session_sync(conn, &session_id))
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    Ok(json!({}))
}

/// Export memories to MEMORY.mira.md (auto memory).
pub async fn write_auto_memory(server: &MiraServer, params: Value) -> Result<Value> {
    let project_id = params
        .get("project_id")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| anyhow::anyhow!("missing required param: project_id"))?;
    let project_path = params
        .get("project_path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing required param: project_path"))?
        .to_string();

    let count = server
        .pool
        .interact(move |conn| {
            if crate::tools::core::claude_local::auto_memory_dir_exists(&project_path) {
                match crate::tools::core::claude_local::write_auto_memory_sync(
                    conn,
                    project_id,
                    &project_path,
                ) {
                    Ok(count) => Ok::<_, anyhow::Error>(count),
                    Err(e) => {
                        tracing::warn!("[mira] Auto memory export failed: {}", e);
                        Ok(0)
                    }
                }
            } else {
                Ok(0)
            }
        })
        .await?;

    Ok(json!({"count": count}))
}

// ═══════════════════════════════════════════════════════════════════════════════
// Phase 4: UserPromptSubmit — composite context gathering
// ═══════════════════════════════════════════════════════════════════════════════

/// Composite op: gather all context needed by the UserPromptSubmit hook.
///
/// Runs server-side so the hook avoids opening its own pools/embeddings.
/// Returns reactive context, proactive insights, team context, and cross-project
/// knowledge in a single response.
pub async fn get_user_prompt_context(server: &MiraServer, params: Value) -> Result<Value> {
    let message = params.get("message").and_then(|v| v.as_str()).unwrap_or("");
    let session_id = params
        .get("session_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // Resolve project
    let (project_id, project_path, _project_name) =
        crate::hooks::resolve_project(&server.pool, Some(session_id).filter(|s| !s.is_empty()))
            .await;

    // Log behavior (fire-and-forget, non-blocking)
    if let Some(pid) = project_id {
        crate::hooks::user_prompt::log_behavior(&server.pool, pid, session_id, message).await;
    }

    // Create ContextInjectionManager from server's shared resources
    let fuzzy = if server.fuzzy_enabled {
        Some(server.fuzzy_cache.clone())
    } else {
        None
    };
    let manager = crate::context::ContextInjectionManager::new(
        server.pool.clone(),
        Some(server.code_pool.clone()),
        server.embeddings.clone(),
        fuzzy,
    )
    .await;

    let config = manager.config().clone();

    // Quality gates: skip proactive/cross-project for simple commands or out-of-bounds length.
    let is_simple = crate::context::is_simple_command(message);
    let msg_len = message.trim().len();
    let in_bounds = msg_len >= config.min_message_len && msg_len <= config.max_message_len;
    let session_opt = if session_id.is_empty() {
        None
    } else {
        Some(session_id)
    };

    let (reactive, proactive, team, cross_project) = tokio::join!(
        manager.get_context_for_message(message, session_id),
        async {
            if let Some(pid) = project_id
                && !is_simple
                && in_bounds
            {
                return crate::hooks::user_prompt::get_proactive_context(
                    &server.pool,
                    pid,
                    project_path.as_deref(),
                    session_opt,
                )
                .await;
            }
            None
        },
        crate::hooks::user_prompt::get_team_context(&server.pool, session_id),
        async {
            if let Some(pid) = project_id
                && !is_simple
            {
                return crate::hooks::user_prompt::get_cross_project_context(
                    &server.pool,
                    &server.embeddings,
                    pid,
                    message,
                )
                .await;
            }
            None
        },
    );

    let sources: Vec<&str> = reactive.sources.iter().map(|s| s.name()).collect();

    Ok(json!({
        "project_id": project_id,
        "project_path": project_path,
        "reactive_context": reactive.context,
        "reactive_sources": sources,
        "reactive_from_cache": reactive.from_cache,
        "reactive_summary": reactive.summary(),
        "reactive_skip_reason": reactive.skip_reason,
        "proactive_context": proactive,
        "team_context": team,
        "cross_project_context": cross_project,
        "config_min_message_len": config.min_message_len,
        "config_max_message_len": config.max_message_len,
        "config_max_chars": config.max_chars,
    }))
}

/// Distill team session knowledge (extract findings from team work).
pub async fn distill_team_session(server: &MiraServer, params: Value) -> Result<Value> {
    let team_id = params
        .get("team_id")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| anyhow::anyhow!("missing required param: team_id"))?;
    let project_id = params.get("project_id").and_then(|v| v.as_i64());

    let result = crate::background::knowledge_distillation::distill_team_session(
        &server.pool,
        team_id,
        project_id,
    )
    .await
    .map_err(|e| anyhow::anyhow!("{e}"))?;

    match result {
        Some(r) => Ok(json!({
            "distilled": true,
            "findings_count": r.findings.len(),
            "team_name": r.team_name,
        })),
        None => Ok(json!({"distilled": false})),
    }
}
