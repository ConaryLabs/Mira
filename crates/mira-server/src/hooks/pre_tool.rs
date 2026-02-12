// crates/mira-server/src/hooks/pre_tool.rs
// PreToolUse hook handler - injects relevant context before Grep/Glob searches

use crate::db::pool::DatabasePool;
use crate::hooks::{
    HookTimer, get_db_path, read_hook_input, resolve_project_id, write_hook_output,
};
use crate::hooks::recall;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

const COOLDOWN_SECS: u64 = 3;
const MAX_RECENT_QUERIES: usize = 5;

#[derive(Serialize, Deserialize, Default)]
struct CooldownState {
    last_fired_at: u64,
    recent_queries: Vec<String>,
}

fn cooldown_path() -> std::path::PathBuf {
    let mira_dir = dirs::home_dir()
        .unwrap_or_default()
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
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string(&state) {
        let _ = std::fs::write(path, json);
    }
}

/// PreToolUse hook input from Claude Code
#[derive(Debug)]
struct PreToolInput {
    tool_name: String,
    pattern: Option<String>,
    path: Option<String>,
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

        Self {
            tool_name: json
                .get("tool_name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            pattern,
            path,
        }
    }
}

/// Run PreToolUse hook
///
/// This hook fires before Grep/Glob tools execute. We:
/// 1. Extract the search pattern
/// 2. Query Mira for relevant memories about that code area
/// 3. Inject context via additionalContext if found
pub async fn run() -> Result<()> {
    let _timer = HookTimer::start("PreToolUse");
    let input = read_hook_input()?;
    let pre_input = PreToolInput::from_json(&input);

    // Only process Grep/Glob/Read operations
    let dominated_tools = ["Grep", "Glob", "Read"];
    if !dominated_tools
        .iter()
        .any(|t| pre_input.tool_name.contains(t))
    {
        write_hook_output(&serde_json::json!({}));
        return Ok(());
    }

    eprintln!(
        "[mira] PreToolUse hook triggered (tool: {}, pattern: {:?})",
        pre_input.tool_name,
        pre_input.pattern.as_deref().unwrap_or("none")
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
            eprintln!("[mira] PreToolUse skipped (cooldown)");
            write_hook_output(&serde_json::json!({}));
            return Ok(());
        }
        if state.recent_queries.contains(&search_query) {
            eprintln!("[mira] PreToolUse skipped (duplicate query)");
            write_hook_output(&serde_json::json!({}));
            return Ok(());
        }
    }

    // Open database
    let db_path = get_db_path();
    let pool = match DatabasePool::open(&db_path).await {
        Ok(p) => Arc::new(p),
        Err(_) => {
            write_hook_output(&serde_json::json!({}));
            return Ok(());
        }
    };

    // Get current project
    let Some(project_id) = resolve_project_id(&pool).await else {
        write_hook_output(&serde_json::json!({}));
        return Ok(());
    };

    // Query for relevant memories (semantic search with keyword fallback)
    let memories = recall::recall_memories(&pool, project_id, &search_query).await;

    // Record this query in cooldown state
    write_cooldown(&search_query);

    // Build output
    let output = if memories.is_empty() {
        serde_json::json!({})
    } else {
        let context = format!(
            "[Mira/memory] Relevant context:\n{}",
            memories.join("\n\n")
        );
        serde_json::json!({
            "hookSpecificOutput": {
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
        };
        assert_eq!(build_search_query(&input), "authentication");
    }

    #[test]
    fn build_query_cleans_regex() {
        let input = PreToolInput {
            tool_name: "Grep".into(),
            pattern: Some("fn\\s+\\w+.*handler".into()),
            path: None,
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
        };
        assert!(build_search_query(&input).is_empty());
    }

    #[test]
    fn build_query_combines_pattern_and_path() {
        let input = PreToolInput {
            tool_name: "Grep".into(),
            pattern: Some("handler".into()),
            path: Some("src/hooks/session.rs".into()),
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
        };
        let result = build_search_query(&input);
        // ".*" becomes " " after cleanup, which trims to empty
        assert!(result.is_empty());
    }
}
