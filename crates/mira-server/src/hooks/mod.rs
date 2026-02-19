// crates/mira-server/src/hooks/mod.rs
// Claude Code hook handlers

pub mod permission;
pub mod post_tool;
pub mod post_tool_failure;
pub mod pre_tool;
pub mod precompact;
pub mod recall;
pub mod session;
pub mod stop;
pub mod subagent;
pub mod task_completed;
pub mod teammate_idle;
pub mod user_prompt;

#[cfg(test)]
mod session_tests;

use anyhow::Result;
use std::io::Read;
use std::path::PathBuf;
use std::time::Instant;

/// Get the Mira database path (~/.mira/mira.db)
pub fn get_db_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| {
        tracing::warn!("HOME directory not set — using current directory for Mira data. This may cause data to be created in your project directory. Consider setting $HOME.");
        PathBuf::from(".")
    });
    home.join(".mira/mira.db")
}

/// Get the Mira code database path (~/.mira/mira-code.db)
pub fn get_code_db_path() -> PathBuf {
    get_db_path().with_file_name("mira-code.db")
}

/// Resolve active project ID, path, and name.
/// Returns (Option<project_id>, Option<project_path>, Option<project_name>).
///
/// Resolution order:
/// 1. Per-session file (`~/.mira/sessions/{session_id}/claude-cwd`) when session_id is provided
/// 2. Global file (`~/.mira/claude-cwd`)
/// 3. Database fallback (`get_last_active_project_sync`)
pub async fn resolve_project(
    pool: &std::sync::Arc<crate::db::pool::DatabasePool>,
    session_id: Option<&str>,
) -> (Option<i64>, Option<String>, Option<String>) {
    // Try per-session cwd first, then global, then DB fallback
    let cwd_from_file = read_session_or_global_cwd(session_id);

    let cwd = cwd_from_file.clone();
    pool.interact(move |conn| {
        // Use file-based cwd if available, otherwise fall back to DB
        let path = cwd.or_else(|| {
            crate::db::get_last_active_project_sync(conn).unwrap_or_else(|e| {
                tracing::warn!("Failed to get last active project: {e}");
                None
            })
        });
        let result = if let Some(ref path) = path {
            crate::db::get_or_create_project_sync(conn, path, None).ok()
        } else {
            None
        };
        match result {
            Some((id, name)) => Ok::<_, anyhow::Error>((Some(id), path, name)),
            None => Ok((None, path, None)),
        }
    })
    .await
    .unwrap_or_else(|e| {
        tracing::warn!("Failed to resolve project: {e}");
        (None, None, None)
    })
}

/// Resolve only the active project ID (convenience wrapper).
pub async fn resolve_project_id(
    pool: &std::sync::Arc<crate::db::pool::DatabasePool>,
    session_id: Option<&str>,
) -> Option<i64> {
    resolve_project(pool, session_id).await.0
}

/// Read cwd from per-session file first, falling back to global file.
///
/// When `session_id` is provided, tries `~/.mira/sessions/{session_id}/claude-cwd` first.
/// Falls back to global `~/.mira/claude-cwd` if the per-session file doesn't exist.
fn read_session_or_global_cwd(session_id: Option<&str>) -> Option<String> {
    if let Some(sid) = session_id {
        // Defense-in-depth: reject empty strings even though callers should filter them
        if sid.is_empty() {
            // Fall through to global cwd
        } else if sid.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            let home = dirs::home_dir()?;
            let per_session_cwd = home.join(format!(".mira/sessions/{}/claude-cwd", sid));
            if let Ok(content) = std::fs::read_to_string(&per_session_cwd) {
                let trimmed = content.trim().to_string();
                if !trimmed.is_empty() {
                    tracing::debug!(
                        "Resolved cwd from per-session file for {}",
                        &sid[..sid.len().min(8)]
                    );
                    return Some(trimmed);
                }
            }
        } else {
            tracing::warn!(
                "Invalid session_id for per-session cwd lookup: contains unsafe characters"
            );
        }
    }

    // Fall back to global cwd file
    crate::hooks::session::read_claude_cwd()
}

/// Performance threshold in milliseconds - warn if hook exceeds this.
/// Note: UserPromptSubmit routinely exceeds this due to embedding lookups.
const HOOK_PERF_THRESHOLD_MS: u128 = 100;

/// Read hook input from stdin (Claude Code passes JSON)
pub fn read_hook_input() -> Result<serde_json::Value> {
    let mut input = String::new();
    std::io::stdin()
        .take(1_048_576)
        .read_to_string(&mut input)?;
    let json: serde_json::Value = serde_json::from_str(&input)?;
    Ok(json)
}

/// Write hook output to stdout
pub fn write_hook_output(output: &serde_json::Value) {
    use std::io::Write;
    match serde_json::to_string(output) {
        Ok(s) => {
            let _ = writeln!(std::io::stdout(), "{}", s);
        }
        Err(e) => {
            eprintln!("Failed to serialize hook output: {}", e);
            let _ = writeln!(std::io::stdout(), "{{}}");
        }
    }
}

/// Get files modified in a session (from Write/Edit/NotebookEdit/MultiEdit tool calls).
/// Shared across session.rs and stop.rs to avoid SQL duplication.
pub fn get_session_modified_files_sync(
    conn: &rusqlite::Connection,
    session_id: &str,
) -> Vec<String> {
    let sql = r#"
        SELECT DISTINCT
            json_extract(arguments, '$.file_path') as file_path
        FROM tool_history
        WHERE session_id = ?
          AND tool_name IN ('Write', 'Edit', 'NotebookEdit', 'MultiEdit')
          AND json_extract(arguments, '$.file_path') IS NOT NULL
        ORDER BY created_at DESC
        LIMIT 10
    "#;
    match conn.prepare(sql) {
        Ok(mut stmt) => stmt
            .query_map([session_id], |row| row.get::<_, String>(0))
            .map(|rows| rows.filter_map(crate::db::log_and_discard).collect())
            .unwrap_or_default(),
        Err(e) => {
            tracing::warn!("Failed to prepare session modified files query: {e}");
            Vec::new()
        }
    }
}

/// Get tool usage stats from behavior log (fallback when tool_history is empty).
/// Returns (total_event_count, top_5_tools_with_counts) from session_behavior_log tool_use events.
pub fn get_behavior_tool_stats_sync(
    conn: &rusqlite::Connection,
    session_id: &str,
) -> (i64, Vec<(String, i64)>) {
    let sql = r#"
        SELECT json_extract(event_data, '$.tool_name') as tool_name, COUNT(*) as cnt
        FROM session_behavior_log
        WHERE session_id = ? AND event_type = 'tool_use'
          AND json_extract(event_data, '$.tool_name') IS NOT NULL
        GROUP BY tool_name
        ORDER BY cnt DESC
    "#;
    match conn.prepare(sql) {
        Ok(mut stmt) => {
            let rows: Vec<(String, i64)> = stmt
                .query_map([session_id], |row| Ok((row.get(0)?, row.get(1)?)))
                .map(|rows| rows.filter_map(crate::db::log_and_discard).collect())
                .unwrap_or_default();
            let total: i64 = rows.iter().map(|(_, c)| c).sum();
            let top_tools: Vec<(String, i64)> = rows.into_iter().take(5).collect();
            (total, top_tools)
        }
        Err(e) => {
            tracing::warn!("Failed to query behavior tool stats: {e}");
            (0, Vec::new())
        }
    }
}

/// Get files modified from behavior log (fallback when tool_history is empty).
/// Looks for file_access events with Write/Edit/NotebookEdit/MultiEdit actions.
pub fn get_behavior_modified_files_sync(
    conn: &rusqlite::Connection,
    session_id: &str,
) -> Vec<String> {
    let sql = r#"
        SELECT DISTINCT json_extract(event_data, '$.file_path')
        FROM session_behavior_log
        WHERE session_id = ?
          AND event_type = 'file_access'
          AND json_extract(event_data, '$.action') IN ('Write', 'Edit', 'NotebookEdit', 'MultiEdit')
          AND json_extract(event_data, '$.file_path') IS NOT NULL
        ORDER BY created_at DESC
        LIMIT 10
    "#;
    match conn.prepare(sql) {
        Ok(mut stmt) => stmt
            .query_map([session_id], |row| row.get::<_, String>(0))
            .map(|rows| rows.filter_map(crate::db::log_and_discard).collect())
            .unwrap_or_default(),
        Err(e) => {
            tracing::warn!("Failed to query behavior modified files: {e}");
            Vec::new()
        }
    }
}

/// Fetch active goals for a project and format them for context injection.
/// Uses `get_active_goals_sync` and returns lines in the format:
///   `"- {title} [{status}] ({progress}%)"`
/// Shared across session, subagent, and stop hooks to avoid duplication.
pub async fn format_active_goals(
    pool: &std::sync::Arc<crate::db::pool::DatabasePool>,
    project_id: i64,
    limit: usize,
) -> Vec<String> {
    let pool_clone = pool.clone();
    pool_clone
        .interact(move |conn| {
            Ok::<_, anyhow::Error>(format_active_goals_sync(conn, project_id, limit))
        })
        .await
        .unwrap_or_default()
}

/// Synchronous version of goal formatting, for use inside `interact` closures.
pub fn format_active_goals_sync(
    conn: &rusqlite::Connection,
    project_id: i64,
    limit: usize,
) -> Vec<String> {
    match crate::db::get_active_goals_sync(conn, Some(project_id), limit) {
        Ok(goals) => goals
            .iter()
            .map(|g| format!("- {} [{}] ({}%)", g.title, g.status, g.progress_percent))
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// Timer guard for hook performance monitoring
/// Logs execution time to stderr on drop
pub struct HookTimer {
    hook_name: &'static str,
    start: Instant,
}

impl HookTimer {
    /// Start timing a hook
    pub fn start(hook_name: &'static str) -> Self {
        Self {
            hook_name,
            start: Instant::now(),
        }
    }
}

impl Drop for HookTimer {
    fn drop(&mut self) {
        let elapsed = self.start.elapsed().as_millis();
        if elapsed > HOOK_PERF_THRESHOLD_MS {
            tracing::warn!(
                "[mira] PERF: {} hook took {}ms (threshold: {}ms)",
                self.hook_name,
                elapsed,
                HOOK_PERF_THRESHOLD_MS
            );
        } else {
            tracing::debug!("[mira] {} hook completed in {}ms", self.hook_name, elapsed);
        }
    }
}
