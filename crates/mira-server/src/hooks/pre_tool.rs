// crates/mira-server/src/hooks/pre_tool.rs
// PreToolUse hook handler - injects relevant context before Grep/Glob searches

use crate::db::pool::DatabasePool;
use crate::hooks::{HookTimer, read_hook_input, write_hook_output};
use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;

/// Get database path
fn get_db_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".mira/mira.db")
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
    let project_id = {
        let pool_clone = pool.clone();
        let result: Result<Option<i64>, _> = pool_clone
            .interact(move |conn| {
                let path = crate::db::get_last_active_project_sync(conn).ok().flatten();
                let result = if let Some(path) = path {
                    crate::db::get_or_create_project_sync(conn, &path, None)
                        .ok()
                        .map(|(id, _)| id)
                } else {
                    None
                };
                Ok::<_, anyhow::Error>(result)
            })
            .await;
        result.ok().flatten()
    };

    let Some(project_id) = project_id else {
        write_hook_output(&serde_json::json!({}));
        return Ok(());
    };

    // Query for relevant memories
    let memories = query_relevant_memories(&pool, project_id, &search_query).await;

    // Build output
    let output = if memories.is_empty() {
        serde_json::json!({})
    } else {
        let context = format!(
            "Relevant context from Mira (based on your search):\n{}",
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
        let path_parts: Vec<&str> = path
            .split('/')
            .filter(|p| !p.is_empty() && *p != "src" && *p != "lib" && *p != ".")
            .collect();
        if let Some(last) = path_parts.last() {
            parts.push(last.to_string());
        }
    }

    parts.join(" ")
}

/// Query Mira for memories relevant to the search
async fn query_relevant_memories(
    pool: &Arc<DatabasePool>,
    project_id: i64,
    query: &str,
) -> Vec<String> {
    let pool_clone = pool.clone();
    let query = query.to_string();

    let result = pool_clone
        .interact(move |conn| {
            // Search for relevant memories using keyword match
            // (semantic search would be better but requires embeddings)
            let sql = r#"
                SELECT content, fact_type, category
                FROM memory_facts
                WHERE project_id = ?
                  AND (content LIKE '%' || ?1 || '%'
                       OR category LIKE '%' || ?1 || '%')
                ORDER BY created_at DESC
                LIMIT 3
            "#;

            let mut stmt = conn.prepare(sql)?;
            let memories: Vec<String> = stmt
                .query_map(rusqlite::params![project_id, query], |row| {
                    let content: String = row.get(0)?;
                    let fact_type: Option<String> = row.get(1)?;
                    let category: Option<String> = row.get(2)?;

                    let prefix = match (fact_type.as_deref(), category.as_deref()) {
                        (Some("decision"), _) => "[Decision]",
                        (Some("preference"), _) => "[Preference]",
                        (_, Some(cat)) => return Ok(format!("[{}] {}", cat, content)),
                        _ => "[Context]",
                    };
                    Ok(format!("{} {}", prefix, content))
                })?
                .filter_map(|r| r.ok())
                .collect();

            Ok::<_, anyhow::Error>(memories)
        })
        .await;

    result.unwrap_or_default()
}
