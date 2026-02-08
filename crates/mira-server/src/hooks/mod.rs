// crates/mira-server/src/hooks/mod.rs
// Claude Code hook handlers

pub mod permission;
pub mod post_tool;
pub mod pre_tool;
pub mod precompact;
pub mod session;
pub mod stop;
pub mod subagent;
pub mod user_prompt;

#[cfg(test)]
mod session_tests;

use anyhow::Result;
use std::path::PathBuf;
use std::time::Instant;

/// Get the Mira database path (~/.mira/mira.db)
pub fn get_db_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".mira/mira.db")
}

/// Resolve active project ID and path in a single DB call.
/// Returns (Option<project_id>, Option<project_path>).
pub async fn resolve_project(
    pool: &std::sync::Arc<crate::db::pool::DatabasePool>,
) -> (Option<i64>, Option<String>) {
    pool.interact(move |conn| {
        let path = crate::db::get_last_active_project_sync(conn).ok().flatten();
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
    .unwrap_or_default()
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
    std::io::Read::read_to_string(&mut std::io::stdin(), &mut input)?;
    let json: serde_json::Value = serde_json::from_str(&input)?;
    Ok(json)
}

/// Write hook output to stdout
pub fn write_hook_output(output: &serde_json::Value) {
    match serde_json::to_string(output) {
        Ok(s) => println!("{}", s),
        Err(e) => eprintln!("Failed to serialize hook output: {}", e),
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
    conn.prepare(sql)
        .ok()
        .and_then(|mut stmt| {
            stmt.query_map([session_id], |row| row.get::<_, String>(0))
                .ok()
                .map(|rows| rows.filter_map(crate::db::log_and_discard).collect())
        })
        .unwrap_or_default()
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
