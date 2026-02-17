// crates/mira-server/src/hooks/pre_tool.rs
// PreToolUse hook handler - injects relevant context before Grep/Glob searches

use crate::hooks::{HookTimer, read_hook_input, write_hook_output};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

const COOLDOWN_SECS: u64 = 3;
const MAX_RECENT_QUERIES: usize = 5;

/// Try to acquire an exclusive lock for the PreToolUse hook.
///
/// When multiple Grep/Glob calls fire in parallel, Claude Code launches a
/// PreToolUse hook for each one simultaneously. Without serialization, all
/// instances race past the cooldown check and call the embedding API, easily
/// exceeding the 2-3s hook timeout.
///
/// This uses a PID-based lock file: write our PID atomically via O_EXCL.
/// If the file already exists and the PID is still alive, another instance
/// is running — return None so the caller can skip immediately. Stale lock
/// files (dead PID) are cleaned up automatically.
fn try_acquire_lock() -> Option<LockGuard> {
    let lock_path = dirs::home_dir()
        .unwrap_or_else(|| {
            eprintln!("[Mira] WARNING: HOME directory not set, using '.' as fallback");
            std::path::PathBuf::from(".")
        })
        .join(".mira")
        .join("tmp")
        .join("pretool.lock");

    if let Some(parent) = lock_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    // Check for existing lock
    if let Ok(contents) = std::fs::read_to_string(&lock_path)
        && let Ok(pid) = contents.trim().parse::<u32>()
    {
        // Check if the process is still alive (cross-platform)
        if is_process_alive(pid) {
            return None; // another instance is running
        }
        // Stale lock — remove it
        let _ = std::fs::remove_file(&lock_path);
    }

    // Try to create lock file exclusively (O_EXCL prevents race between check and create)
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts.open(&lock_path).ok()?;
    let _ = write!(file, "{}", std::process::id());

    Some(LockGuard { path: lock_path })
}

/// Check if a process with the given PID is still alive.
/// On Unix, uses `kill -0` which works on both Linux and macOS.
/// On non-Unix platforms, returns false (assumes stale), which is the safe
/// default -- it just means parallel hooks can fire without serialization.
fn is_process_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

/// RAII guard that removes the lock file on drop.
struct LockGuard {
    path: std::path::PathBuf,
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[derive(Serialize, Deserialize, Default)]
struct CooldownState {
    last_fired_at: u64,
    recent_queries: Vec<String>,
}

fn cooldown_path() -> std::path::PathBuf {
    let mira_dir = dirs::home_dir()
        .unwrap_or_else(|| {
            eprintln!("[Mira] WARNING: HOME directory not set, using '.' as fallback");
            std::path::PathBuf::from(".")
        })
        .join(".mira")
        .join("tmp");
    mira_dir.join("pretool_last.json")
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn read_cooldown() -> Option<CooldownState> {
    let data = std::fs::read_to_string(cooldown_path()).ok()?;
    serde_json::from_str(&data).ok()
}

fn write_cooldown(query: &str) {
    let mut state = read_cooldown().unwrap_or_default();
    state.last_fired_at = unix_now();
    state.recent_queries.push(query.to_string());
    if state.recent_queries.len() > MAX_RECENT_QUERIES {
        state.recent_queries.remove(0);
    }
    let path = cooldown_path();
    if let Some(parent) = path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        tracing::debug!("Failed to create cooldown dir: {e}");
    }
    if let Ok(json) = serde_json::to_string(&state) {
        // Write to temp file then rename for atomicity (prevents corruption on crash)
        let temp = path.with_extension("tmp");
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let file = opts.open(&temp);
        if let Ok(mut f) = file {
            if let Err(e) = f.write_all(json.as_bytes()) {
                tracing::debug!("Failed to write cooldown temp file: {e}");
                return;
            }
            drop(f);
            if let Err(e) = std::fs::rename(&temp, &path) {
                tracing::debug!("Failed to rename cooldown temp file: {e}");
            }
        }
    }
}

/// PreToolUse hook input from Claude Code
#[derive(Debug)]
struct PreToolInput {
    tool_name: String,
    pattern: Option<String>,
    path: Option<String>,
    /// File path for Edit/Write tools (extracted from tool_input.file_path)
    file_path: Option<String>,
}

impl PreToolInput {
    fn from_json(json: &serde_json::Value) -> Self {
        let tool_input = json.get("tool_input");

        // Extract search pattern from Grep or Glob
        let pattern = tool_input
            .and_then(|ti| {
                ti.get("pattern")
                    .or_else(|| ti.get("query"))
                    .or_else(|| ti.get("regex"))
            })
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let path = tool_input
            .and_then(|ti| ti.get("path"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // Extract file_path for Edit/Write tools
        let file_path = tool_input
            .and_then(|ti| ti.get("file_path"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Self {
            tool_name: json
                .get("tool_name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            pattern,
            path,
            file_path,
        }
    }
}

/// Run PreToolUse hook
///
/// This hook fires before Grep/Glob/Read/Edit/Write tools execute. We:
/// 1. For Grep/Glob/Read: extract search pattern and inject relevant memories
/// 2. For Edit/Write: check if the target file is a known change hotspot and warn
pub async fn run() -> Result<()> {
    let input = read_hook_input().context("Failed to parse hook input from stdin")?;
    let pre_input = PreToolInput::from_json(&input);

    // Handle Edit/Write: check for change pattern warnings (fast, no embeddings)
    if pre_input.tool_name == "Edit" || pre_input.tool_name == "Write" {
        return handle_edit_write_patterns(&input, &pre_input).await;
    }

    // Only process Grep/Glob/Read operations
    let dominated_tools = ["Grep", "Glob", "Read"];
    if !dominated_tools
        .iter()
        .any(|t| pre_input.tool_name.contains(t))
    {
        write_hook_output(&serde_json::json!({}));
        return Ok(());
    }

    // Serialize parallel invocations: if another PreToolUse hook is already
    // running (e.g., 8 Grep calls fired in parallel), skip immediately rather
    // than all racing to call the embedding API and timing out.
    let _lock = match try_acquire_lock() {
        Some(lock) => lock,
        None => {
            tracing::debug!("PreToolUse skipped (another instance running)");
            write_hook_output(&serde_json::json!({}));
            return Ok(());
        }
    };

    let _timer = HookTimer::start("PreToolUse");

    tracing::debug!(
        tool = %pre_input.tool_name,
        pattern = pre_input.pattern.as_deref().unwrap_or("none"),
        "PreToolUse hook triggered"
    );

    // Build search query from pattern and path
    let search_query = build_search_query(&pre_input);
    if search_query.is_empty() {
        write_hook_output(&serde_json::json!({}));
        return Ok(());
    }

    // Cooldown and dedup check
    if let Some(state) = read_cooldown() {
        let now = unix_now();
        if now - state.last_fired_at < COOLDOWN_SECS {
            tracing::debug!("PreToolUse skipped (cooldown)");
            write_hook_output(&serde_json::json!({}));
            return Ok(());
        }
        if state.recent_queries.contains(&search_query) {
            tracing::debug!("PreToolUse skipped (duplicate query)");
            write_hook_output(&serde_json::json!({}));
            return Ok(());
        }
    }

    // Connect to MCP server via IPC (falls back to direct DB if server unavailable)
    let mut client = crate::ipc::client::HookClient::connect().await;
    tracing::debug!(
        backend = if client.is_ipc() { "IPC" } else { "direct" },
        "PreToolUse using backend"
    );

    // Get current project
    let Some((project_id, _path)) = client.resolve_project(None).await else {
        write_hook_output(&serde_json::json!({}));
        return Ok(());
    };

    // Query for relevant memories (semantic search with keyword fallback)
    let memories = client.recall_memories(project_id, &search_query).await;

    // Record this query in cooldown state
    write_cooldown(&search_query);

    // Build output
    let output = if memories.is_empty() {
        serde_json::json!({})
    } else {
        let context = format!("[Mira/memory] Relevant context:\n{}", memories.join("\n\n"));
        serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "additionalContext": context
            }
        })
    };

    write_hook_output(&output);
    Ok(())
}

/// Build a search query from the tool input
fn build_search_query(input: &PreToolInput) -> String {
    let mut parts = Vec::new();

    if let Some(pattern) = &input.pattern {
        // Clean up regex patterns for semantic search
        let cleaned = pattern
            .replace(".*", " ")
            .replace("\\s+", " ")
            .replace("\\w+", " ")
            .replace("[^/]+", " ")
            .replace("\\", "")
            .replace("^", "")
            .replace("$", "");
        if !cleaned.trim().is_empty() {
            parts.push(cleaned.trim().to_string());
        }
    }

    if let Some(path) = &input.path {
        // Extract meaningful parts from path
        let path_parts: Vec<String> = Path::new(path)
            .components()
            .filter_map(|c| {
                let s = c.as_os_str().to_str()?;
                if s == "src" || s == "lib" || s == "." {
                    None
                } else {
                    Some(s.to_string())
                }
            })
            .collect();
        if let Some(last) = path_parts.last() {
            parts.push(last.to_string());
        }
    }

    parts.join(" ")
}

/// Handle Edit/Write tools: check if the target file is a known change hotspot.
///
/// Queries `behavior_patterns` for `change_pattern` entries whose `pattern_data`
/// mentions the target file path. Only does a simple SQL query (no embeddings)
/// to stay within the hook timeout.
async fn handle_edit_write_patterns(
    _input: &serde_json::Value,
    pre_input: &PreToolInput,
) -> Result<()> {
    let file_path = match &pre_input.file_path {
        Some(fp) if !fp.is_empty() => fp.clone(),
        _ => {
            write_hook_output(&serde_json::json!({}));
            return Ok(());
        }
    };

    let _timer = HookTimer::start("PreToolUse:pattern_check");

    // Open DB directly (lightweight, no embeddings needed)
    let db_path = crate::hooks::get_db_path();
    let pool = match crate::db::pool::DatabasePool::open_hook(&db_path).await {
        Ok(p) => p,
        Err(_) => {
            write_hook_output(&serde_json::json!({}));
            return Ok(());
        }
    };

    // Resolve project
    let (project_id, _) = crate::hooks::resolve_project(&std::sync::Arc::new(pool)).await;
    let Some(project_id) = project_id else {
        write_hook_output(&serde_json::json!({}));
        return Ok(());
    };

    // Query for change patterns that mention this file
    let fp = file_path.clone();
    let pool2 = {
        let db_path = crate::hooks::get_db_path();
        match crate::db::pool::DatabasePool::open_hook(&db_path).await {
            Ok(p) => std::sync::Arc::new(p),
            Err(_) => {
                write_hook_output(&serde_json::json!({}));
                return Ok(());
            }
        }
    };

    let warnings: Vec<String> = pool2
        .interact(move |conn| {
            let sql = r#"
                SELECT pattern_data, occurrence_count
                FROM behavior_patterns
                WHERE project_id = ?1
                  AND pattern_type = 'change_pattern'
                  AND pattern_data LIKE ?2
                ORDER BY occurrence_count DESC
                LIMIT 3
            "#;
            // Use the filename (not full path) for broader matching
            let filename = Path::new(&fp)
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or(&fp);
            let like_pattern = format!("%{}%", filename);

            let mut stmt = match conn.prepare(sql) {
                Ok(s) => s,
                Err(_) => return Ok::<_, anyhow::Error>(Vec::new()),
            };
            let rows = stmt
                .query_map(rusqlite::params![project_id, like_pattern], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                })
                .map(|rows| {
                    rows.filter_map(|r| r.ok()).collect::<Vec<_>>()
                })
                .unwrap_or_default();

            let mut warnings = Vec::new();
            for (pattern_data_str, occurrence_count) in rows {
                if let Some(data) =
                    crate::proactive::patterns::PatternData::from_json(&pattern_data_str)
                {
                    if let crate::proactive::patterns::PatternData::ChangePattern {
                        pattern_subtype,
                        outcome_stats,
                        ..
                    } = data
                    {
                        let warning = match pattern_subtype.as_str() {
                            "module_hotspot" => format!(
                                "hotspot: modified {} times, {}/{} changes needed follow-up fixes",
                                occurrence_count,
                                outcome_stats.follow_up_fix,
                                outcome_stats.total,
                            ),
                            "size_risk" => format!(
                                "size risk: {}/{} changes to this area needed follow-up fixes",
                                outcome_stats.follow_up_fix, outcome_stats.total,
                            ),
                            "co_change_gap" => format!(
                                "co-change pattern: this file is usually changed with related files ({}/{} had issues when changed alone)",
                                outcome_stats.follow_up_fix, outcome_stats.total,
                            ),
                            other => format!(
                                "{}: modified {} times",
                                other, occurrence_count,
                            ),
                        };
                        warnings.push(warning);
                    }
                }
            }
            Ok(warnings)
        })
        .await
        .unwrap_or_default();

    let output = if warnings.is_empty() {
        serde_json::json!({})
    } else {
        let context = format!(
            "[Mira/patterns] \u{26a0} {}",
            warnings.join("; "),
        );
        serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "additionalContext": context
            }
        })
    };

    write_hook_output(&output);
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // ── PreToolInput::from_json ─────────────────────────────────────────────

    #[test]
    fn pre_input_parses_full_input() {
        let input = PreToolInput::from_json(&serde_json::json!({
            "tool_name": "Grep",
            "tool_input": {
                "pattern": "fn main",
                "path": "/home/user/project/src"
            }
        }));
        assert_eq!(input.tool_name, "Grep");
        assert_eq!(input.pattern.as_deref(), Some("fn main"));
        assert_eq!(input.path.as_deref(), Some("/home/user/project/src"));
    }

    #[test]
    fn pre_input_defaults_on_empty_json() {
        let input = PreToolInput::from_json(&serde_json::json!({}));
        assert!(input.tool_name.is_empty());
        assert!(input.pattern.is_none());
        assert!(input.path.is_none());
        assert!(input.file_path.is_none());
    }

    #[test]
    fn pre_input_extracts_query_field() {
        let input = PreToolInput::from_json(&serde_json::json!({
            "tool_name": "Search",
            "tool_input": {
                "query": "authentication handler"
            }
        }));
        assert_eq!(input.pattern.as_deref(), Some("authentication handler"));
    }

    #[test]
    fn pre_input_extracts_regex_field() {
        let input = PreToolInput::from_json(&serde_json::json!({
            "tool_name": "Grep",
            "tool_input": {
                "regex": "fn\\s+\\w+"
            }
        }));
        assert_eq!(input.pattern.as_deref(), Some("fn\\s+\\w+"));
    }

    #[test]
    fn pre_input_prefers_pattern_over_query() {
        let input = PreToolInput::from_json(&serde_json::json!({
            "tool_name": "Grep",
            "tool_input": {
                "pattern": "primary",
                "query": "secondary"
            }
        }));
        assert_eq!(input.pattern.as_deref(), Some("primary"));
    }

    #[test]
    fn pre_input_ignores_wrong_types() {
        let input = PreToolInput::from_json(&serde_json::json!({
            "tool_name": 999,
            "tool_input": {
                "pattern": 42,
                "path": true
            }
        }));
        assert!(input.tool_name.is_empty());
        assert!(input.pattern.is_none());
        assert!(input.path.is_none());
    }

    #[test]
    fn pre_input_missing_tool_input() {
        let input = PreToolInput::from_json(&serde_json::json!({
            "tool_name": "Glob"
        }));
        assert_eq!(input.tool_name, "Glob");
        assert!(input.pattern.is_none());
        assert!(input.path.is_none());
    }

    // ── build_search_query ──────────────────────────────────────────────────

    #[test]
    fn build_query_from_pattern_only() {
        let input = PreToolInput {
            tool_name: "Grep".into(),
            pattern: Some("authentication".into()),
            path: None,
            file_path: None,
        };
        assert_eq!(build_search_query(&input), "authentication");
    }

    #[test]
    fn build_query_cleans_regex() {
        let input = PreToolInput {
            tool_name: "Grep".into(),
            pattern: Some("fn\\s+\\w+.*handler".into()),
            path: None,
            file_path: None,
        };
        let result = build_search_query(&input);
        assert!(!result.contains("\\s+"));
        assert!(!result.contains("\\w+"));
        assert!(!result.contains(".*"));
        assert!(result.contains("fn"));
        assert!(result.contains("handler"));
    }

    #[test]
    fn build_query_cleans_anchors() {
        let input = PreToolInput {
            tool_name: "Grep".into(),
            pattern: Some("^pub fn$".into()),
            path: None,
            file_path: None,
        };
        let result = build_search_query(&input);
        assert!(!result.contains('^'));
        assert!(!result.contains('$'));
        assert!(result.contains("pub fn"));
    }

    #[test]
    fn build_query_extracts_path_component() {
        let input = PreToolInput {
            tool_name: "Glob".into(),
            pattern: None,
            path: Some("src/hooks/session.rs".into()),
            file_path: None,
        };
        let result = build_search_query(&input);
        assert_eq!(result, "session.rs");
    }

    #[test]
    fn build_query_skips_common_dirs() {
        let input = PreToolInput {
            tool_name: "Glob".into(),
            pattern: None,
            path: Some("./src/lib".into()),
            file_path: None,
        };
        let result = build_search_query(&input);
        // ".", "src", and "lib" are all filtered out, so result might be empty
        // or contain only meaningful parts
        assert!(!result.contains("src"));
    }

    #[test]
    fn build_query_empty_input() {
        let input = PreToolInput {
            tool_name: "Grep".into(),
            pattern: None,
            path: None,
            file_path: None,
        };
        assert!(build_search_query(&input).is_empty());
    }

    #[test]
    fn build_query_combines_pattern_and_path() {
        let input = PreToolInput {
            tool_name: "Grep".into(),
            pattern: Some("handler".into()),
            path: Some("src/hooks/session.rs".into()),
            file_path: None,
        };
        let result = build_search_query(&input);
        assert!(result.contains("handler"));
        assert!(result.contains("session.rs"));
    }

    #[test]
    fn build_query_whitespace_only_pattern_ignored() {
        let input = PreToolInput {
            tool_name: "Grep".into(),
            pattern: Some(".*".into()),
            path: None,
            file_path: None,
        };
        let result = build_search_query(&input);
        // ".*" becomes " " after cleanup, which trims to empty
        assert!(result.is_empty());
    }

    // ── Edit/Write file_path extraction ───────────────────────────────────

    #[test]
    fn pre_input_extracts_file_path_for_edit() {
        let input = PreToolInput::from_json(&serde_json::json!({
            "tool_name": "Edit",
            "tool_input": {
                "file_path": "/home/user/project/src/main.rs",
                "old_string": "foo",
                "new_string": "bar"
            }
        }));
        assert_eq!(input.tool_name, "Edit");
        assert_eq!(
            input.file_path.as_deref(),
            Some("/home/user/project/src/main.rs")
        );
    }

    #[test]
    fn pre_input_extracts_file_path_for_write() {
        let input = PreToolInput::from_json(&serde_json::json!({
            "tool_name": "Write",
            "tool_input": {
                "file_path": "/home/user/project/new_file.rs",
                "content": "fn main() {}"
            }
        }));
        assert_eq!(input.tool_name, "Write");
        assert_eq!(
            input.file_path.as_deref(),
            Some("/home/user/project/new_file.rs")
        );
    }

    #[test]
    fn pre_input_no_file_path_for_grep() {
        let input = PreToolInput::from_json(&serde_json::json!({
            "tool_name": "Grep",
            "tool_input": {
                "pattern": "fn main",
                "path": "/home/user/project/src"
            }
        }));
        assert!(input.file_path.is_none());
    }
}
