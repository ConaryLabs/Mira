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
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".mira/mira.db")
}

/// Get the Mira code database path (~/.mira/mira-code.db)
pub fn get_code_db_path() -> PathBuf {
    get_db_path().with_file_name("mira-code.db")
}

/// Resolve active project ID and path in a single DB call.
/// Returns (Option<project_id>, Option<project_path>).
pub async fn resolve_project(
    pool: &std::sync::Arc<crate::db::pool::DatabasePool>,
) -> (Option<i64>, Option<String>) {
    pool.interact(move |conn| {
        let path = crate::db::get_last_active_project_sync(conn).unwrap_or_else(|e| {
            tracing::warn!("Failed to get last active project: {e}");
            None
        });
        let result = if let Some(ref path) = path {
            crate::db::get_or_create_project_sync(conn, path, None)
                .ok()
                .map(|(id, _)| id)
        } else {
            None
        };
        Ok::<_, anyhow::Error>((result, path))
    })
    .await
    .unwrap_or_else(|e| {
        tracing::warn!("Failed to resolve project: {e}");
        (None, None)
    })
}

/// Resolve only the active project ID (convenience wrapper).
pub async fn resolve_project_id(
    pool: &std::sync::Arc<crate::db::pool::DatabasePool>,
) -> Option<i64> {
    resolve_project(pool).await.0
}

/// Performance threshold in milliseconds - warn if hook exceeds this
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
            eprintln!(
                "[mira] PERF WARNING: {} hook took {}ms (threshold: {}ms)",
                self.hook_name, elapsed, HOOK_PERF_THRESHOLD_MS
            );
        } else {
            eprintln!("[mira] {} hook completed in {}ms", self.hook_name, elapsed);
        }
    }
}
