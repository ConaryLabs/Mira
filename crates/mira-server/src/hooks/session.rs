// crates/mira-server/src/hooks/session.rs
// SessionStart hook handler - captures Claude Code's session_id and cwd

use crate::db::pool::DatabasePool;
use crate::ipc::client::HookClient;
use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use super::get_db_path;

/// File where Claude's session_id is stored for MCP to read
pub fn session_file_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".mira/claude-session-id")
}

/// File where Claude's working directory is stored for MCP to read
pub fn cwd_file_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".mira/claude-cwd")
}

/// File where Claude's session source info is stored for MCP to read
pub fn source_file_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".mira/claude-source.json")
}

/// File where Claude's task list ID is stored for MCP to read
pub fn task_list_file_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".mira/claude-task-list-id")
}

/// Marker file path for tracking whether goals were already shown this session.
/// Other hooks can check this to avoid re-injecting goals.
fn goals_shown_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".mira/tmp/goals_shown")
}

/// Write a file with restricted permissions (0o600 on Unix).
/// Used for sensitive session files (session ID, cwd, task list ID, goals marker).
fn write_file_restricted(path: &std::path::Path, content: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(path)?;
    f.write_all(content.as_bytes())
}

/// Mark that goals have been injected into the session context.
/// Called by SessionStart after injecting goals, so other hooks can skip them.
fn mark_goals_shown() {
    let path = goals_shown_path();
    if let Some(parent) = path.parent()
        && let Err(e) = fs::create_dir_all(parent)
    {
        tracing::debug!("Failed to create goals_shown dir: {e}");
    }
    if let Err(e) = write_file_restricted(&path, &Utc::now().to_rfc3339()) {
        tracing::debug!("Failed to write goals_shown marker: {e}");
    }
}

/// Check whether goals have already been shown this session.
/// Returns true if goals were shown within the last 30 minutes
/// (stale markers from crashed sessions are ignored).
pub fn were_goals_shown() -> bool {
    let path = goals_shown_path();
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return false,
    };
    // Parse the timestamp and check if it's recent (within 30 minutes)
    if let Ok(ts) = chrono::DateTime::parse_from_rfc3339(content.trim()) {
        let age = Utc::now().signed_duration_since(ts);
        age.num_minutes() < 30
    } else {
        false
    }
}

/// Source information captured from SessionStart hook
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SourceInfo {
    pub session_id: Option<String>,
    pub source: String,
    pub timestamp: String,
}

/// Team membership info cached per-session to avoid cross-session clobbering.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamMembership {
    pub team_id: i64,
    pub team_name: String,
    pub member_name: String,
    pub role: String,
    pub config_path: String,
}

impl SourceInfo {
    pub fn new(session_id: Option<String>, source: &str) -> Self {
        Self {
            session_id,
            source: source.to_string(),
            timestamp: Utc::now().to_rfc3339(),
        }
    }
}

/// Handle SessionStart hook from Claude Code
/// Extracts session_id, cwd, and source from stdin JSON and writes to files
/// On resume, injects context about previous session work
pub async fn run() -> Result<()> {
    let input = super::read_hook_input().context("Failed to parse hook input from stdin")?;

    // Log hook input keys for debugging
    tracing::debug!(
        keys = ?input.as_object().map(|obj| obj.keys().collect::<Vec<_>>()),
        "SessionStart hook input keys"
    );

    // Ensure .mira directory exists
    let mira_dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".mira");
    fs::create_dir_all(&mira_dir)?;

    // Extract session_id from Claude's hook input
    let session_id = input.get("session_id").and_then(|v| v.as_str());
    if let Some(sid) = session_id {
        let path = session_file_path();
        write_file_restricted(&path, sid)?;
        tracing::debug!(session_id = sid, "Captured Claude session");
    }

    // Extract cwd from Claude's hook input for auto-project detection
    let cwd = input.get("cwd").and_then(|v| v.as_str());
    if let Some(cwd_val) = cwd {
        let path = cwd_file_path();
        write_file_restricted(&path, cwd_val)?;
        tracing::debug!(cwd = cwd_val, "Captured Claude cwd");
    }

    // Determine session source (startup vs resume)
    // Claude Code passes "resumed" or similar flag when using --resume
    let source = input
        .get("source")
        .and_then(|v| v.as_str())
        .or_else(|| {
            // Check for resumed flag as fallback
            if input
                .get("resumed")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                Some("resume")
            } else {
                None
            }
        })
        .unwrap_or("startup");

    // Write source info atomically (temp file + rename)
    let source_info = SourceInfo::new(session_id.map(String::from), source);
    let source_json = serde_json::to_string(&source_info)?;
    let source_path = source_file_path();
    let temp_path = source_path.with_extension("tmp");
    fs::write(&temp_path, &source_json)?;
    fs::rename(&temp_path, &source_path)?;
    tracing::debug!(source = source, "Captured Claude source");

    // Extract task_list_id from Claude's hook input or env var
    let task_list_id = input
        .get("task_list_id")
        .and_then(|v| v.as_str())
        .map(String::from)
        .or_else(|| std::env::var("CLAUDE_CODE_TASK_LIST_ID").ok());

    if let Some(ref list_id) = task_list_id {
        let path = task_list_file_path();
        write_file_restricted(&path, list_id)?;
        tracing::debug!(task_list_id = %list_id, "Captured Claude task list");
    }

    // Connect to MCP server via IPC (falls back to direct DB)
    let mut client = HookClient::connect().await;

    // Register session in DB so the background loop sees activity
    if let (Some(sid), Some(cwd_val)) = (session_id, cwd) {
        match client.register_session(sid, cwd_val, source).await {
            Some(pid) => tracing::debug!(project_id = pid, "Session registered in DB"),
            None => tracing::warn!("Failed to register session"),
        }
    }

    // Detect team membership and register in DB
    if let Some(sid) = session_id {
        let detection = detect_team_membership(&input, Some(sid), cwd);
        if let Some(det) = detection {
            tracing::info!(
                team = %det.team_name,
                role = %det.role,
                member = %det.member_name,
                "Team detected"
            );

            let membership = async {
                let team_id = client
                    .register_team_session(
                        &det.team_name,
                        &det.config_path,
                        &det.member_name,
                        &det.role,
                        det.agent_type.as_deref(),
                        sid,
                        cwd,
                    )
                    .await?;
                Some(TeamMembership {
                    team_id,
                    team_name: det.team_name.clone(),
                    member_name: det.member_name.clone(),
                    role: det.role.clone(),
                    config_path: det.config_path.clone(),
                })
            }
            .await;

            if let Some(ref m) = membership {
                let _ = write_team_membership(sid, m);
                tracing::debug!(team_id = m.team_id, "Team session registered");
            }
        }
    }

    // Inject context about previous work via IPC
    let cwd_owned = cwd.map(String::from);
    let session_id_owned = session_id.map(String::from);

    let context = if source == "resume" {
        client
            .get_resume_context(cwd_owned.as_deref(), session_id_owned.as_deref())
            .await
    } else {
        client.get_startup_context(cwd_owned.as_deref()).await
    };

    if let Some(ctx) = context {
        let output = serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "SessionStart",
                "additionalContext": ctx
            }
        });
        super::write_hook_output(&output);
    } else {
        super::write_hook_output(&serde_json::json!({}));
    }
    Ok(())
}

/// Build lightweight context for a fresh startup session.
/// Includes active goals and a brief note about the last session.
pub(crate) async fn build_startup_context(
    cwd: Option<&str>,
    pool: Option<Arc<DatabasePool>>,
) -> Option<String> {
    let pool = match pool {
        Some(p) => p,
        None => {
            let db_path = get_db_path();
            match DatabasePool::open_hook(&db_path).await {
                Ok(p) => Arc::new(p),
                Err(_) => return None,
            }
        }
    };

    let project_id: Option<i64> = if let Some(cwd_path) = cwd {
        let pool_clone = pool.clone();
        let cwd_owned = cwd_path.to_string();
        pool_clone
            .interact(move |conn| {
                Ok::<_, anyhow::Error>(
                    crate::db::get_or_create_project_sync(conn, &cwd_owned, None)
                        .ok()
                        .map(|(id, _)| id),
                )
            })
            .await
            .ok()
            .flatten()
    } else {
        super::resolve_project_id(&pool).await
    };
    let project_id = project_id?;

    let mut context_parts: Vec<String> = Vec::new();

    // Brief note about last session (summary only, not detailed tool history)
    let pool_clone = pool.clone();
    let previous_session: Option<crate::db::SessionInfo> = pool_clone
        .interact(move |conn| {
            Ok::<_, anyhow::Error>(
                crate::db::get_recent_sessions_sync(conn, project_id, 2)
                    .ok()
                    .and_then(|sessions| sessions.into_iter().find(|s| s.status != "active")),
            )
        })
        .await
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
    let goal_lines = super::format_active_goals(&pool, project_id, 5).await;
    if !goal_lines.is_empty() {
        context_parts.push(format!(
            "[Mira/goals] Active goals:\n{}",
            goal_lines.join("\n")
        ));
        mark_goals_shown();
    }

    if context_parts.is_empty() {
        return None;
    }

    Some(context_parts.join("\n\n"))
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
            let db_path = get_db_path();
            match DatabasePool::open_hook(&db_path).await {
                Ok(p) => Arc::new(p),
                Err(_) => return None,
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
                        .ok()
                        .map(|(id, _)| id),
                )
            })
            .await
            .ok()
            .flatten()
    } else {
        // Fallback to last active project only if no cwd available
        super::resolve_project_id(&pool).await
    };
    let project_id = project_id?;

    let mut context_parts: Vec<String> = Vec::new();

    // Get the most recent completed session for this project
    let pool_clone = pool.clone();
    let previous_session: Option<crate::db::SessionInfo> = pool_clone
        .interact(move |conn| {
            Ok::<_, anyhow::Error>(
                crate::db::get_recent_sessions_sync(conn, project_id, 2)
                    .ok()
                    .and_then(|sessions| {
                        // Find the most recent non-active session
                        sessions.into_iter().find(|s| s.status != "active")
                    }),
            )
        })
        .await
        .ok()
        .flatten();

    // Get recent tool calls and modified files from previous session
    if let Some(ref prev_session) = previous_session {
        // Fetch last 5 tool calls
        let pool_clone = pool.clone();
        let prev_id = prev_session.id.clone();
        let tool_history: Option<Vec<crate::db::ToolHistoryEntry>> = pool_clone
            .interact(move |conn| {
                Ok::<_, anyhow::Error>(crate::db::get_session_history_sync(conn, &prev_id, 5).ok())
            })
            .await
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
                Ok::<_, anyhow::Error>(super::get_session_modified_files_sync(conn, &prev_id))
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

            // Surface pre-compaction context if available
            if let Some(compaction_summary) = build_compaction_summary(&snap) {
                context_parts.push(compaction_summary);
            }
        }
    }

    // Get incomplete goals
    let goal_lines = super::format_active_goals(&pool, project_id, 3).await;
    if !goal_lines.is_empty() {
        context_parts.push(format!(
            "[Mira/goals] Active goals:\n{}",
            goal_lines.join("\n")
        ));
        mark_goals_shown();
    }

    // Add team context if in a team
    let team_membership = if let Some(sid) = session_id {
        read_team_membership_from_db(&pool, sid).await
    } else {
        read_team_membership()
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

    Some(format!(
        "**Resuming session** - Here's context from your previous work:\n\n{}",
        context_parts.join("\n\n")
    ))
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
/// Returns a formatted string with decisions, pending tasks, issues, and active work,
/// or None if no compaction context is present or all categories are empty.
pub(crate) fn build_compaction_summary(snapshot: &serde_json::Value) -> Option<String> {
    let cc = snapshot.get("compaction_context")?;
    let mut parts: Vec<String> = Vec::new();

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

    if let Some(pending) = cc.get("pending_tasks").and_then(|v| v.as_array()) {
        let items: Vec<&str> = pending.iter().filter_map(|d| d.as_str()).take(3).collect();
        if !items.is_empty() {
            parts.push(format!("Pending: {}", items.join("; ")));
        }
    }

    if let Some(issues) = cc.get("issues").and_then(|v| v.as_array()) {
        let items: Vec<&str> = issues.iter().filter_map(|d| d.as_str()).take(3).collect();
        if !items.is_empty() {
            parts.push(format!("Issues: {}", items.join("; ")));
        }
    }

    if let Some(active) = cc.get("active_work").and_then(|v| v.as_array()) {
        let items: Vec<&str> = active.iter().filter_map(|d| d.as_str()).take(1).collect();
        if !items.is_empty() {
            parts.push(format!("Active work: {}", items[0]));
        }
    }

    if parts.is_empty() {
        return None;
    }

    Some(format!("**Pre-compaction context:**\n{}", parts.join("\n")))
}

/// Infer a human-readable activity description from the most-used tools
fn infer_activity_from_tools(tools: &[&str]) -> String {
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

/// Read Claude's session_id from the temp file (if available)
pub fn read_claude_session_id() -> Option<String> {
    let path = session_file_path();
    fs::read_to_string(&path).ok().map(|s| s.trim().to_string())
}

/// Read Claude's working directory from the temp file (if available)
pub fn read_claude_cwd() -> Option<String> {
    let path = cwd_file_path();
    fs::read_to_string(&path).ok().map(|s| s.trim().to_string())
}

/// Read source info from the JSON file (if available)
pub fn read_source_info() -> Option<SourceInfo> {
    let path = source_file_path();
    let content = fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Read Claude's task list ID from the temp file (if available)
pub fn read_claude_task_list_id() -> Option<String> {
    let path = task_list_file_path();
    fs::read_to_string(&path).ok().map(|s| s.trim().to_string())
}

// ═══════════════════════════════════════════════════════════════════════════════
// TEAM DETECTION
// ═══════════════════════════════════════════════════════════════════════════════

/// Per-session team membership file (avoids cross-session clobbering).
pub fn team_file_path_for_session(session_id: &str) -> Option<PathBuf> {
    if !session_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        tracing::warn!(
            "Invalid characters in session_id for team file path, skipping: {:?}",
            session_id
        );
        return None;
    }
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    Some(home.join(format!(".mira/claude-team-{}.json", session_id)))
}

/// Read team membership for the current session (filesystem-based).
/// Prefer `read_team_membership_from_db` when a pool and session_id are available.
pub fn read_team_membership() -> Option<TeamMembership> {
    let session_id = read_claude_session_id()?;
    let path = team_file_path_for_session(&session_id)?;
    let content = fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Read team membership from DB for a specific session (session-isolated).
/// DB is the sole source of truth — no filesystem fallback, which could
/// revive stale membership from crashed/partially-cleaned sessions.
pub async fn read_team_membership_from_db(
    pool: &std::sync::Arc<crate::db::pool::DatabasePool>,
    session_id: &str,
) -> Option<TeamMembership> {
    if session_id.is_empty() {
        return None;
    }
    let pool_clone = pool.clone();
    let sid = session_id.to_string();
    pool_clone
        .interact(move |conn| {
            Ok::<_, anyhow::Error>(crate::db::get_team_membership_for_session_sync(conn, &sid))
        })
        .await
        .ok()
        .flatten()
}

/// Write team membership atomically (temp + rename) with restricted permissions (0o600).
pub fn write_team_membership(session_id: &str, membership: &TeamMembership) -> Result<()> {
    let path = team_file_path_for_session(session_id)
        .ok_or_else(|| anyhow::anyhow!("Invalid session_id for team file path: {session_id:?}"))?;
    let json = serde_json::to_string(membership)?;
    let temp_path = path.with_extension("tmp");

    // Write temp file with restricted permissions (0o600)
    {
        use std::io::Write;
        let mut opts = fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&temp_path)?;
        f.write_all(json.as_bytes())?;
    }

    fs::rename(&temp_path, &path)?;
    Ok(())
}

/// Clean up per-session team file.
pub fn cleanup_team_file(session_id: &str) {
    let Some(path) = team_file_path_for_session(session_id) else {
        return;
    };
    let _ = fs::remove_file(&path);
}

/// Detect team membership from Claude Code's Agent Teams config files.
///
/// Scans `~/.claude/teams/*/config.json` for team configs that reference
/// the current session or working directory. Also checks the SessionStart
/// input for an `agent_type` field.
pub fn detect_team_membership(
    input: &serde_json::Value,
    session_id: Option<&str>,
    cwd: Option<&str>,
) -> Option<TeamDetectionResult> {
    // Primary: Check SessionStart input for agent_type (Claude Code provides this)
    let agent_type = input.get("agent_type").and_then(|v| v.as_str());
    let member_name = input
        .get("agent_name")
        .and_then(|v| v.as_str())
        .or_else(|| input.get("member_name").and_then(|v| v.as_str()));

    // If agent_type is set, this is a team member
    if let Some(agent_type) = agent_type {
        // Try to find the team config
        if let Some(team_config) = scan_team_configs(cwd) {
            let role = if agent_type == "lead" {
                "lead"
            } else {
                "teammate"
            };
            return Some(TeamDetectionResult {
                team_name: team_config.team_name,
                config_path: team_config.config_path,
                member_name: member_name.unwrap_or(agent_type).to_string(),
                role: role.to_string(),
                agent_type: Some(agent_type.to_string()),
            });
        }
    }

    // Secondary: Scan filesystem for team configs
    if let Some(team_config) = scan_team_configs(cwd) {
        // Derive member name from session_id or config
        let name = member_name
            .map(|s| s.to_string())
            .or_else(|| session_id.map(|s| format!("member-{}", &s[..8.min(s.len())])))
            .unwrap_or_else(|| "unknown".to_string());

        return Some(TeamDetectionResult {
            team_name: team_config.team_name,
            config_path: team_config.config_path,
            member_name: name,
            role: "teammate".to_string(),
            agent_type: agent_type.map(String::from),
        });
    }

    None
}

/// Result of team detection
pub struct TeamDetectionResult {
    pub team_name: String,
    pub config_path: String,
    pub member_name: String,
    pub role: String,
    pub agent_type: Option<String>,
}

struct TeamConfigInfo {
    team_name: String,
    config_path: String,
}

/// Scan `~/.claude/teams/*/config.json` for team configs.
/// When multiple teams match, prefer the most specific project_path
/// (longest path that is still an ancestor of cwd) for determinism.
fn scan_team_configs(cwd: Option<&str>) -> Option<TeamConfigInfo> {
    let home = dirs::home_dir()?;
    let teams_dir = home.join(".claude/teams");

    if !teams_dir.is_dir() {
        return None;
    }

    let entries = fs::read_dir(&teams_dir).ok()?;
    let mut candidates: Vec<(usize, TeamConfigInfo)> = Vec::new();
    let mut fallback: Vec<TeamConfigInfo> = Vec::new();

    for entry in entries.flatten() {
        let config_path = entry.path().join("config.json");
        if !config_path.is_file() {
            continue;
        }

        let content = match fs::read_to_string(&config_path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let config: serde_json::Value = match serde_json::from_str(&content) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let team_name_val = config
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| entry.file_name().to_string_lossy().to_string());

        // Check if cwd is under the team's project path (not the reverse —
        // matching proj_p.starts_with(cwd_p) would be overly broad, e.g.
        // cwd="/home/peter" would match project="/home/peter/Mira").
        if let Some(project_path) = config.get("project_path").and_then(|v| v.as_str())
            && let Some(cwd_val) = cwd
        {
            let cwd_p = std::path::Path::new(cwd_val);
            let proj_p = std::path::Path::new(project_path);
            if cwd_p.starts_with(proj_p) {
                let specificity = proj_p.components().count();
                candidates.push((
                    specificity,
                    TeamConfigInfo {
                        team_name: team_name_val,
                        config_path: config_path.to_string_lossy().to_string(),
                    },
                ));
                continue;
            }
        }

        // If we don't have cwd, collect as fallback (but prefer cwd matches)
        if cwd.is_none() {
            fallback.push(TeamConfigInfo {
                team_name: team_name_val,
                config_path: config_path.to_string_lossy().to_string(),
            });
        }
    }

    if !candidates.is_empty() {
        // Most specific project path wins; tie-break on team name, then config path
        candidates.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then_with(|| a.1.team_name.cmp(&b.1.team_name))
                .then_with(|| a.1.config_path.cmp(&b.1.config_path))
        });
        return candidates.into_iter().next().map(|(_, info)| info);
    }

    if !fallback.is_empty() {
        // Deterministic: sort by team name, then config path for full tie-break
        fallback.sort_by(|a, b| {
            a.team_name
                .cmp(&b.team_name)
                .then_with(|| a.config_path.cmp(&b.config_path))
        });
        return fallback.into_iter().next();
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================================
    // read_claude_session_id tests
    // ============================================================================

    #[test]
    fn test_read_claude_session_id_trims_whitespace() {
        use tempfile::TempDir;

        // Create a temp directory with custom session file
        let temp_dir = TempDir::new().unwrap();
        let session_path = temp_dir.path().join("claude-session-id");

        // Write session ID with whitespace
        std::fs::write(&session_path, "  session123\n  ").unwrap();

        // Read directly from the file (since read_claude_session_id uses fixed path)
        let content = std::fs::read_to_string(&session_path)
            .ok()
            .map(|s| s.trim().to_string());

        assert_eq!(content, Some("session123".to_string()));
    }

    // ============================================================================
    // read_claude_cwd tests
    // ============================================================================

    #[test]
    fn test_read_claude_cwd_trims_whitespace() {
        use tempfile::TempDir;

        // Create a temp directory with custom cwd file
        let temp_dir = TempDir::new().unwrap();
        let cwd_path = temp_dir.path().join("claude-cwd");

        // Write cwd with whitespace
        std::fs::write(&cwd_path, "  /home/user/project\n  ").unwrap();

        // Read directly from the file (since read_claude_cwd uses fixed path)
        let content = std::fs::read_to_string(&cwd_path)
            .ok()
            .map(|s| s.trim().to_string());

        assert_eq!(content, Some("/home/user/project".to_string()));
    }

    // ============================================================================
    // build_working_on_summary tests
    // ============================================================================

    #[test]
    fn test_build_working_on_summary_with_edit_tools_and_files() {
        let snapshot = serde_json::json!({
            "tool_count": 15,
            "top_tools": [
                {"name": "Edit", "count": 8},
                {"name": "Read", "count": 5},
            ],
            "files_modified": ["/home/user/project/src/main.rs", "/home/user/project/src/lib.rs"],
        });
        let result = build_working_on_summary(&snapshot);
        assert!(result.is_some());
        let summary = result.unwrap();
        assert!(summary.contains("code editing"), "got: {}", summary);
        assert!(summary.contains("main.rs"), "got: {}", summary);
        assert!(summary.contains("lib.rs"), "got: {}", summary);
    }

    #[test]
    fn test_build_working_on_summary_with_bash_tools() {
        let snapshot = serde_json::json!({
            "tool_count": 5,
            "top_tools": [
                {"name": "Bash", "count": 4},
            ],
            "files_modified": [],
        });
        let result = build_working_on_summary(&snapshot);
        assert!(result.is_some());
        assert!(result.unwrap().contains("running commands"));
    }

    #[test]
    fn test_build_working_on_summary_empty_snapshot() {
        let snapshot = serde_json::json!({
            "tool_count": 0,
            "top_tools": [],
            "files_modified": [],
        });
        let result = build_working_on_summary(&snapshot);
        assert!(result.is_none());
    }

    #[test]
    fn test_build_working_on_summary_fallback_to_tool_count() {
        let snapshot = serde_json::json!({
            "tool_count": 10,
            "top_tools": [
                {"name": "SomeUnknownTool", "count": 10},
            ],
            "files_modified": [],
        });
        let result = build_working_on_summary(&snapshot);
        assert!(result.is_some());
        assert!(result.unwrap().contains("10 tool calls"));
    }

    // ============================================================================
    // infer_activity_from_tools tests
    // ============================================================================

    #[test]
    fn test_infer_activity_edit() {
        assert_eq!(infer_activity_from_tools(&["Edit", "Read"]), "code editing");
    }

    #[test]
    fn test_infer_activity_write() {
        assert_eq!(infer_activity_from_tools(&["Write"]), "code editing");
    }

    #[test]
    fn test_infer_activity_bash() {
        assert_eq!(infer_activity_from_tools(&["Bash"]), "running commands");
    }

    #[test]
    fn test_infer_activity_exploration() {
        assert_eq!(
            infer_activity_from_tools(&["Read", "Glob"]),
            "code exploration"
        );
    }

    #[test]
    fn test_infer_activity_unknown() {
        assert_eq!(infer_activity_from_tools(&["SomeTool"]), "");
    }

    // ============================================================================
    // build_compaction_summary tests
    // ============================================================================

    #[test]
    fn test_compaction_summary_all_categories() {
        let snapshot = serde_json::json!({
            "tool_count": 10,
            "compaction_context": {
                "decisions": ["chose builder pattern for Config"],
                "pending_tasks": ["add validation for user input"],
                "issues": ["connection refused when connecting to database"],
                "active_work": ["working on database migration"]
            }
        });
        let result = build_compaction_summary(&snapshot);
        assert!(result.is_some());
        let summary = result.unwrap();
        assert!(
            summary.contains("Pre-compaction context:"),
            "got: {}",
            summary
        );
        assert!(summary.contains("Decisions:"), "got: {}", summary);
        assert!(summary.contains("builder pattern"), "got: {}", summary);
        assert!(summary.contains("Pending:"), "got: {}", summary);
        assert!(summary.contains("Issues:"), "got: {}", summary);
        assert!(summary.contains("Active work:"), "got: {}", summary);
    }

    #[test]
    fn test_compaction_summary_partial_categories() {
        let snapshot = serde_json::json!({
            "compaction_context": {
                "decisions": ["chose SQLite"],
                "pending_tasks": [],
                "issues": [],
                "active_work": []
            }
        });
        let result = build_compaction_summary(&snapshot);
        assert!(result.is_some());
        let summary = result.unwrap();
        assert!(summary.contains("Decisions:"), "got: {}", summary);
        assert!(!summary.contains("Pending:"), "got: {}", summary);
        assert!(!summary.contains("Issues:"), "got: {}", summary);
    }

    #[test]
    fn test_compaction_summary_none_when_absent() {
        let snapshot = serde_json::json!({
            "tool_count": 5,
            "top_tools": [],
        });
        let result = build_compaction_summary(&snapshot);
        assert!(result.is_none());
    }

    #[test]
    fn test_compaction_summary_none_when_all_empty() {
        let snapshot = serde_json::json!({
            "compaction_context": {
                "decisions": [],
                "pending_tasks": [],
                "issues": [],
                "active_work": []
            }
        });
        let result = build_compaction_summary(&snapshot);
        assert!(result.is_none());
    }

    #[test]
    fn test_compaction_summary_limits_items() {
        let snapshot = serde_json::json!({
            "compaction_context": {
                "decisions": ["d1", "d2", "d3", "d4", "d5"],
                "pending_tasks": [],
                "issues": [],
                "active_work": []
            }
        });
        let result = build_compaction_summary(&snapshot);
        assert!(result.is_some());
        let summary = result.unwrap();
        // take(3) limits to 3 decisions shown
        assert!(summary.contains("d1"), "got: {}", summary);
        assert!(summary.contains("d3"), "got: {}", summary);
        assert!(!summary.contains("d4"), "got: {}", summary);
    }
}
