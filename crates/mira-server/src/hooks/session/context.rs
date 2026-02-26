// crates/mira-server/src/hooks/session/context.rs
//! Session context builders: startup and resume context injection.

use crate::db::pool::DatabasePool;
use crate::utils::truncate_at_boundary;
use std::sync::Arc;

/// Hard cap on SessionStart output to prevent bloated context injection.
/// A loaded resume with compaction context + goals + incomplete tasks can
/// exceed 3000 chars without this limit.
const MAX_SESSION_CONTEXT_CHARS: usize = 2500;

/// Build lightweight context for a fresh startup session.
/// Includes active goals and a brief note about the last session.
/// `session_id` is used for per-session goals-shown tracking.
pub(crate) async fn build_startup_context(
    cwd: Option<&str>,
    pool: Option<Arc<DatabasePool>>,
    session_id: Option<&str>,
) -> Option<String> {
    let pool = match pool {
        Some(p) => p,
        None => {
            let db_path = crate::hooks::get_db_path();
            match DatabasePool::open_hook(&db_path).await {
                Ok(p) => Arc::new(p),
                Err(e) => {
                    tracing::warn!("Failed to open DB for startup context: {e}");
                    return None;
                }
            }
        }
    };

    let (project_id, is_new_project): (Option<i64>, bool) = if let Some(cwd_path) = cwd {
        let pool_clone = pool.clone();
        // Normalize before checking existence to match the path that get_or_create_project_sync
        // will INSERT, preventing false-positive "new project" when the same directory is
        // referenced via different path forms (symlink, trailing slash, tilde).
        let cwd_owned = crate::utils::normalize_project_path(cwd_path);
        let result: Option<(i64, bool)> = pool_clone
            .interact(move |conn| {
                // Check if the project already exists before upserting
                let existing: Option<i64> = conn
                    .query_row(
                        "SELECT id FROM projects WHERE path = ?",
                        [&cwd_owned as &str],
                        |row| row.get(0),
                    )
                    .ok();
                let is_new = existing.is_none();
                Ok::<_, anyhow::Error>(
                    crate::db::get_or_create_project_sync(conn, &cwd_owned, None)
                        .map_err(|e| {
                            tracing::debug!("context load: get_or_create_project failed: {e}")
                        })
                        .ok()
                        .map(|(id, _)| (id, is_new)),
                )
            })
            .await
            .map_err(|e| tracing::debug!("context load: project interact failed: {e}"))
            .ok()
            .flatten();
        match result {
            Some((id, is_new)) => (Some(id), is_new),
            None => (None, false),
        }
    } else {
        (
            crate::hooks::resolve_project_id(&pool, session_id).await,
            false,
        )
    };
    let project_id = project_id?;

    let mut context_parts: Vec<String> = Vec::new();

    // Surface a message when Mira encounters a new project directory
    if is_new_project {
        let project_name = cwd
            .and_then(|p| std::path::Path::new(p).file_name())
            .and_then(|f| f.to_str())
            .unwrap_or("unknown");
        context_parts.push(format!(
            "[Mira] New project detected: {}. Memories and goals are scoped to this project.",
            project_name
        ));
    }

    // Brief note about last session (summary only, not detailed tool history)
    let pool_clone = pool.clone();
    let previous_session: Option<crate::db::SessionInfo> = pool_clone
        .interact(move |conn| {
            Ok::<_, anyhow::Error>(
                crate::db::get_recent_sessions_sync(conn, project_id, 2)
                    .map_err(|e| tracing::debug!("context load: get_recent_sessions failed: {e}"))
                    .ok()
                    .and_then(|sessions| sessions.into_iter().find(|s| s.status != "active")),
            )
        })
        .await
        .map_err(|e| tracing::debug!("context load: sessions interact failed: {e}"))
        .ok()
        .flatten();

    if let Some(ref prev_session) = previous_session {
        if let Some(ref summary) = prev_session.summary {
            context_parts.push(format!("**Last session:** {}", summary));
        }

        // Check snapshot for a brief "you were working on" note
        let pool_clone = pool.clone();
        let prev_id = prev_session.id.clone();
        let snapshot: Option<String> = pool_clone
            .interact(move |conn| Ok::<_, anyhow::Error>(get_session_snapshot_sync(conn, &prev_id)))
            .await
            .map_err(|e| tracing::debug!("context load: snapshot interact failed: {e}"))
            .ok()
            .flatten();

        if let Some(snapshot_json) = snapshot
            && let Ok(snap) = serde_json::from_str::<serde_json::Value>(&snapshot_json)
            && let Some(working_on) = build_working_on_summary(&snap)
            && context_parts.is_empty()
        {
            // Only add if we didn't already have a summary
            context_parts.push(format!("**Last session:** {}", working_on));
        }
    }

    // Active goals
    let goal_lines = crate::hooks::format_active_goals(&pool, project_id, 5).await;
    if !goal_lines.is_empty() {
        context_parts.push(format!(
            "[Mira/goals] Active goals:\n{}",
            goal_lines.join("\n")
        ));
        super::mark_goals_shown(session_id);
    }

    if context_parts.is_empty() {
        if previous_session.is_none() {
            // First-ever session for this user — show a welcome message
            return Some(
                "[Mira] Welcome! Mira is now tracking this project. Try /mira:recap for context, or /mira:remember to store your first decision.".to_string()
            );
        }
        return None;
    }

    let output = context_parts.join("\n\n");
    Some(truncate_session_context(output))
}

/// Build context for a resumed session
pub(crate) async fn build_resume_context(
    cwd: Option<&str>,
    session_id: Option<&str>,
    pool: Option<Arc<DatabasePool>>,
) -> Option<String> {
    let pool = match pool {
        Some(p) => p,
        None => {
            let db_path = crate::hooks::get_db_path();
            match DatabasePool::open_hook(&db_path).await {
                Ok(p) => Arc::new(p),
                Err(e) => {
                    tracing::warn!("Failed to open DB for resume context: {e}");
                    return None;
                }
            }
        }
    };

    // Resolve project from cwd (current working directory) to ensure we get
    // context for the right project, not whatever was last active globally.
    let project_id: Option<i64> = if let Some(cwd_path) = cwd {
        let pool_clone = pool.clone();
        let cwd_owned = cwd_path.to_string();
        pool_clone
            .interact(move |conn| {
                Ok::<_, anyhow::Error>(
                    crate::db::get_or_create_project_sync(conn, &cwd_owned, None)
                        .map_err(|e| {
                            tracing::debug!("context load: get_or_create_project failed: {e}")
                        })
                        .ok()
                        .map(|(id, _)| id),
                )
            })
            .await
            .map_err(|e| tracing::debug!("context load: project interact failed: {e}"))
            .ok()
            .flatten()
    } else {
        // Fallback to last active project only if no cwd available
        crate::hooks::resolve_project_id(&pool, session_id).await
    };
    let project_id = project_id?;

    let mut context_parts: Vec<String> = Vec::new();

    // Get the most recent completed session for this project
    let pool_clone = pool.clone();
    let previous_session: Option<crate::db::SessionInfo> = pool_clone
        .interact(move |conn| {
            Ok::<_, anyhow::Error>(
                crate::db::get_recent_sessions_sync(conn, project_id, 2)
                    .map_err(|e| tracing::debug!("context load: get_recent_sessions failed: {e}"))
                    .ok()
                    .and_then(|sessions| {
                        // Find the most recent non-active session
                        sessions.into_iter().find(|s| s.status != "active")
                    }),
            )
        })
        .await
        .map_err(|e| tracing::debug!("context load: sessions interact failed: {e}"))
        .ok()
        .flatten();

    // Get recent tool calls and modified files from previous session
    if let Some(ref prev_session) = previous_session {
        // Fetch last 5 tool calls
        let pool_clone = pool.clone();
        let prev_id = prev_session.id.clone();
        let tool_history: Option<Vec<crate::db::ToolHistoryEntry>> = pool_clone
            .interact(move |conn| {
                Ok::<_, anyhow::Error>(
                    crate::db::get_session_history_sync(conn, &prev_id, 5)
                        .map_err(|e| {
                            tracing::debug!("context load: get_session_history failed: {e}")
                        })
                        .ok(),
                )
            })
            .await
            .map_err(|e| tracing::debug!("context load: tool history interact failed: {e}"))
            .ok()
            .flatten();

        if let Some(history) = tool_history.filter(|h| !h.is_empty()) {
            let tool_lines: Vec<String> = history
                .iter()
                .rev() // Oldest first
                .map(|h| {
                    let status = if h.success { "ok" } else { "err" };
                    let summary = h
                        .result_summary
                        .as_deref()
                        .map(|s| {
                            if s.len() > 80 {
                                format!("{}...", crate::utils::truncate_at_boundary(s, 80))
                            } else {
                                s.to_string()
                            }
                        })
                        .unwrap_or_default();
                    format!("  [{}] {} -> {}", status, h.tool_name, summary)
                })
                .collect();
            context_parts.push(format!(
                "**Last session's recent actions:**\n{}",
                tool_lines.join("\n")
            ));
        }

        // Fetch files modified in the previous session (Write/Edit/NotebookEdit tool calls)
        let pool_clone = pool.clone();
        let prev_id = prev_session.id.clone();
        let modified_files: Vec<String> = pool_clone
            .interact(move |conn| {
                Ok::<_, anyhow::Error>(crate::hooks::get_session_modified_files_sync(
                    conn, &prev_id,
                ))
            })
            .await
            .unwrap_or_default();

        if !modified_files.is_empty() {
            let file_names: Vec<&str> = modified_files
                .iter()
                .map(|p| {
                    std::path::Path::new(p.as_str())
                        .file_name()
                        .and_then(|f| f.to_str())
                        .unwrap_or(p)
                })
                .collect();
            let files_str = if file_names.len() <= 5 {
                file_names.join(", ")
            } else {
                format!(
                    "{} (+{} more)",
                    file_names[..5].join(", "),
                    file_names.len() - 5
                )
            };
            context_parts.push(format!("**Files modified last session:** {}", files_str));
        }

        // Add session summary if available
        if let Some(ref summary) = prev_session.summary {
            context_parts.push(format!("**Previous session summary:** {}", summary));
        }

        // Check for a stored session snapshot (structured metadata from stop hook)
        let pool_clone = pool.clone();
        let prev_id = prev_session.id.clone();
        let snapshot: Option<String> = pool_clone
            .interact(move |conn| Ok::<_, anyhow::Error>(get_session_snapshot_sync(conn, &prev_id)))
            .await
            .map_err(|e| tracing::debug!("context load: snapshot interact failed: {e}"))
            .ok()
            .flatten();

        if let Some(snapshot_json) = snapshot
            && let Ok(snap) = serde_json::from_str::<serde_json::Value>(&snapshot_json)
        {
            // Build "You were working on X" from snapshot data
            if let Some(working_on) = build_working_on_summary(&snap) {
                // Insert at the beginning for prominence
                context_parts.insert(0, format!("**You were working on:** {}", working_on));
            }

            // Surface pre-compaction context right after "working on" for prominence.
            // This is the richest signal from the previous session.
            if let Some(compaction_summary) = build_compaction_summary(&snap) {
                // Position after "working on" (index 0) if it exists, otherwise at top
                let insert_pos = context_parts
                    .iter()
                    .position(|p| p.starts_with("**You were working on:"))
                    .map_or(0, |i| i + 1);
                context_parts.insert(insert_pos, compaction_summary);
            }
        }

        // Fetch incomplete tasks from previous session
        let pool_clone = pool.clone();
        let prev_id = prev_session.id.clone();
        let incomplete_tasks: Vec<crate::db::session_tasks::IncompleteTask> = pool_clone
            .interact(move |conn| {
                Ok::<_, anyhow::Error>(
                    crate::db::session_tasks::get_incomplete_tasks_for_session_sync(conn, &prev_id),
                )
            })
            .await
            .unwrap_or_default();

        if !incomplete_tasks.is_empty() {
            let subjects: Vec<&str> = incomplete_tasks
                .iter()
                .map(|t| t.subject.as_str())
                .take(10)
                .collect();
            context_parts.push(format!(
                "**Previous session had {} incomplete task(s):** {}",
                incomplete_tasks.len(),
                subjects.join(", ")
            ));
        }
    }

    // Get incomplete goals
    let goal_lines = crate::hooks::format_active_goals(&pool, project_id, 3).await;
    if !goal_lines.is_empty() {
        context_parts.push(format!(
            "[Mira/goals] Active goals:\n{}",
            goal_lines.join("\n")
        ));
        super::mark_goals_shown(session_id);
    }

    // Add team context if in a team
    let team_membership = if let Some(sid) = session_id {
        super::read_team_membership_from_db(&pool, sid).await
    } else {
        super::read_team_membership()
    };
    if let Some(membership) = team_membership {
        let pool_clone = pool.clone();
        let tid = membership.team_id;
        let members: Vec<crate::db::TeamMemberInfo> = pool_clone
            .interact(move |conn| {
                Ok::<_, anyhow::Error>(crate::db::get_active_team_members_sync(conn, tid))
            })
            .await
            .unwrap_or_default();

        let other_members: Vec<&str> = members
            .iter()
            .filter(|m| m.member_name != membership.member_name)
            .map(|m| m.member_name.as_str())
            .collect();

        let team_line = if other_members.is_empty() {
            format!(
                "**Team:** {} (you are {}, no other active teammates)",
                membership.team_name, membership.member_name
            )
        } else {
            format!(
                "**Team:** {} (you are {}, active teammates: {})",
                membership.team_name,
                membership.member_name,
                other_members.join(", ")
            )
        };
        context_parts.push(team_line);
    }

    if context_parts.is_empty() {
        return None;
    }

    let output = format!(
        "**Resuming session** - Here's context from your previous work:\n\n{}",
        context_parts.join("\n\n")
    );
    Some(truncate_session_context(output))
}

// get_session_modified_files_sync is now in hooks/mod.rs

/// Get session snapshot metadata stored by the stop hook
pub(crate) fn get_session_snapshot_sync(
    conn: &rusqlite::Connection,
    session_id: &str,
) -> Option<String> {
    conn.query_row(
        "SELECT snapshot FROM session_snapshots WHERE session_id = ?",
        [session_id],
        |row| row.get::<_, String>(0),
    )
    .ok()
}

/// Build a "You were working on X" summary from snapshot data
pub(crate) fn build_working_on_summary(snapshot: &serde_json::Value) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();

    // Top tools used gives a hint of what they were doing
    if let Some(top_tools) = snapshot.get("top_tools").and_then(|v| v.as_array()) {
        let tool_names: Vec<&str> = top_tools
            .iter()
            .filter_map(|t| t.get("name").and_then(|n| n.as_str()))
            .take(3)
            .collect();
        if !tool_names.is_empty() {
            let activity = infer_activity_from_tools(&tool_names);
            if !activity.is_empty() {
                parts.push(activity);
            }
        }
    }

    // Modified files
    if let Some(files) = snapshot.get("files_modified").and_then(|v| v.as_array()) {
        let file_names: Vec<&str> = files
            .iter()
            .filter_map(|f| f.as_str())
            .filter_map(|p| std::path::Path::new(p).file_name().and_then(|f| f.to_str()))
            .take(3)
            .collect();
        if !file_names.is_empty() {
            parts.push(format!("editing {}", file_names.join(", ")));
        }
    }

    if parts.is_empty() {
        // Fall back to tool count
        if let Some(count) = snapshot.get("tool_count").and_then(|v| v.as_i64())
            && count > 0
        {
            return Some(format!("{} tool calls in the previous session", count));
        }
        return None;
    }

    Some(parts.join(", "))
}

/// Build a summary of pre-compaction context from the snapshot's `compaction_context` field.
///
/// Returns a formatted string with user intent, decisions, active work, issues, pending tasks,
/// and files referenced — ordered most-valuable-first.
/// Returns None if no compaction context is present or all categories are empty.
pub(crate) fn build_compaction_summary(snapshot: &serde_json::Value) -> Option<String> {
    let cc = snapshot.get("compaction_context")?;
    let mut parts: Vec<String> = Vec::new();

    // User intent (most valuable - what they were trying to accomplish)
    if let Some(intent) = cc.get("user_intent").and_then(|v| v.as_str())
        && !intent.is_empty()
    {
        parts.push(format!("Original request: {}", intent));
    }

    if let Some(decisions) = cc.get("decisions").and_then(|v| v.as_array()) {
        let items: Vec<&str> = decisions
            .iter()
            .filter_map(|d| d.as_str())
            .take(3)
            .collect();
        if !items.is_empty() {
            parts.push(format!("Decisions: {}", items.join("; ")));
        }
    }

    if let Some(active) = cc.get("active_work").and_then(|v| v.as_array()) {
        let items: Vec<&str> = active.iter().filter_map(|d| d.as_str()).take(1).collect();
        if !items.is_empty() {
            parts.push(format!("Active work: {}", items[0]));
        }
    }

    if let Some(issues) = cc.get("issues").and_then(|v| v.as_array()) {
        let items: Vec<&str> = issues.iter().filter_map(|d| d.as_str()).take(3).collect();
        if !items.is_empty() {
            parts.push(format!("Issues: {}", items.join("; ")));
        }
    }

    if let Some(pending) = cc.get("pending_tasks").and_then(|v| v.as_array()) {
        let items: Vec<&str> = pending.iter().filter_map(|d| d.as_str()).take(3).collect();
        if !items.is_empty() {
            parts.push(format!("Remaining tasks: {}", items.join("; ")));
        }
    }

    // Files referenced (compact, comma-separated, up to 8)
    if let Some(files) = cc.get("files_referenced").and_then(|v| v.as_array()) {
        let items: Vec<&str> = files.iter().filter_map(|f| f.as_str()).take(8).collect();
        if !items.is_empty() {
            parts.push(format!("Files discussed: {}", items.join(", ")));
        }
    }

    // Structured findings from expert analysis or code review
    if let Some(findings) = cc.get("findings").and_then(|v| v.as_array()) {
        let items: Vec<&str> = findings.iter().filter_map(|f| f.as_str()).take(3).collect();
        if !items.is_empty() {
            parts.push(format!("Key findings:\n{}", items.join("\n")));
        }
    }

    if parts.is_empty() {
        return None;
    }

    Some(format!("**Pre-compaction context:**\n{}", parts.join("\n")))
}

/// Infer a human-readable activity description from the most-used tools
pub(super) fn infer_activity_from_tools(tools: &[&str]) -> String {
    // Map tool names to activity descriptions
    let has = |name: &str| tools.iter().any(|t| t.eq_ignore_ascii_case(name));

    if has("Edit") || has("Write") {
        "code editing".to_string()
    } else if has("Bash") {
        "running commands".to_string()
    } else if has("Read") || has("Glob") || has("Grep") {
        "code exploration".to_string()
    } else if has("mcp__mira__code") || has("code") || has("mcp__mira__diff") || has("diff") {
        "code analysis".to_string()
    } else if has("mcp__mira__memory") || has("memory") {
        "memory operations".to_string()
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn build_compaction_summary_includes_findings() {
        let snapshot = json!({
            "compaction_context": {
                "findings": ["Bug in auth handler", "Missing index on users table"]
            }
        });
        let result = build_compaction_summary(&snapshot).unwrap();
        assert!(
            result.contains("Key findings:"),
            "should contain Key findings header"
        );
        assert!(
            result.contains("Bug in auth handler"),
            "should contain first finding"
        );
        assert!(
            result.contains("Missing index on users table"),
            "should contain second finding"
        );
    }

    #[test]
    fn build_compaction_summary_returns_none_for_empty() {
        // No compaction_context at all
        let snapshot = json!({});
        assert!(
            build_compaction_summary(&snapshot).is_none(),
            "missing compaction_context should return None"
        );

        // compaction_context present but all fields empty
        let snapshot = json!({
            "compaction_context": {
                "decisions": [],
                "active_work": [],
                "issues": [],
                "pending_tasks": [],
                "files_referenced": [],
                "findings": []
            }
        });
        assert!(
            build_compaction_summary(&snapshot).is_none(),
            "empty compaction_context fields should return None"
        );
    }

    #[test]
    fn build_compaction_summary_includes_user_intent() {
        let snapshot = json!({
            "compaction_context": {
                "user_intent": "Refactor the auth module to use middleware"
            }
        });
        let result = build_compaction_summary(&snapshot).unwrap();
        assert!(
            result.contains("Original request: Refactor the auth module to use middleware"),
            "should render user_intent as Original request"
        );
    }
}

/// Enforce hard output budget on session context.
/// Truncates at a UTF-8 safe boundary and appends `\n...` if over the limit.
/// The final output is guaranteed to be <= MAX_SESSION_CONTEXT_CHARS.
fn truncate_session_context(output: String) -> String {
    if output.len() <= MAX_SESSION_CONTEXT_CHARS {
        return output;
    }
    const SUFFIX: &str = "\n...";
    let budget = MAX_SESSION_CONTEXT_CHARS.saturating_sub(SUFFIX.len());
    let mut truncated = truncate_at_boundary(&output, budget).to_string();
    // Avoid mid-line truncation by finding the last newline
    if let Some(pos) = truncated.rfind('\n') {
        truncated.truncate(pos);
    }
    truncated.push_str(SUFFIX);
    truncated
}
