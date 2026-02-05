// crates/mira-server/src/hooks/subagent.rs
// SubagentStart and SubagentStop hook handlers

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

/// SubagentStart hook input
#[derive(Debug)]
struct SubagentStartInput {
    subagent_type: String,
    task_description: Option<String>,
}

impl SubagentStartInput {
    fn from_json(json: &serde_json::Value) -> Self {
        Self {
            subagent_type: json
                .get("subagent_type")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string(),
            task_description: json
                .get("task_description")
                .or_else(|| json.get("prompt"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
        }
    }
}

/// Run SubagentStart hook
///
/// Injects relevant Mira context when a subagent spawns:
/// 1. Active goals related to current work
/// 2. Recent decisions about relevant code areas
/// 3. Key memories that might help the subagent
pub async fn run_start() -> Result<()> {
    let _timer = HookTimer::start("SubagentStart");
    let input = read_hook_input()?;
    let start_input = SubagentStartInput::from_json(&input);

    eprintln!(
        "[mira] SubagentStart hook triggered (type: {}, task: {:?})",
        start_input.subagent_type,
        start_input
            .task_description
            .as_deref()
            .map(|s| if s.len() > 50 {
                format!("{}...", &s[..50])
            } else {
                s.to_string()
            })
    );

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

    let mut context_parts: Vec<String> = Vec::new();

    // Get active goals
    let goals = get_active_goals(&pool, project_id).await;
    if !goals.is_empty() {
        context_parts.push(format!("**Active Goals:**\n{}", goals.join("\n")));
    }

    // Get relevant memories based on task description
    if let Some(task) = &start_input.task_description {
        let memories = get_relevant_memories(&pool, project_id, task).await;
        if !memories.is_empty() {
            context_parts.push(format!("**Relevant Context:**\n{}", memories.join("\n")));
        }
    }

    // Build output
    let output = if context_parts.is_empty() {
        serde_json::json!({})
    } else {
        let context = format!(
            "Mira context for this subagent:\n\n{}",
            context_parts.join("\n\n")
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

/// Run SubagentStop hook
///
/// Captures useful discoveries from subagent work
pub async fn run_stop() -> Result<()> {
    let _timer = HookTimer::start("SubagentStop");
    let input = read_hook_input()?;

    eprintln!("[mira] SubagentStop hook triggered");

    // For now, just acknowledge - could extract patterns from subagent output
    // in a future enhancement
    let _subagent_output = input
        .get("subagent_output")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // Simple passthrough for now
    write_hook_output(&serde_json::json!({}));
    Ok(())
}

/// Get active goals for context injection
async fn get_active_goals(pool: &Arc<DatabasePool>, project_id: i64) -> Vec<String> {
    let pool_clone = pool.clone();

    let result = pool_clone
        .interact(move |conn| {
            let sql = r#"
                SELECT title, status, progress_percent
                FROM goals
                WHERE project_id = ?
                  AND status IN ('planning', 'in_progress')
                ORDER BY
                    CASE priority
                        WHEN 'critical' THEN 1
                        WHEN 'high' THEN 2
                        WHEN 'medium' THEN 3
                        ELSE 4
                    END,
                    created_at DESC
                LIMIT 3
            "#;

            let mut stmt = conn.prepare(sql)?;
            let goals: Vec<String> = stmt
                .query_map(rusqlite::params![project_id], |row| {
                    let title: String = row.get(0)?;
                    let status: String = row.get(1)?;
                    let progress: i32 = row.get(2)?;
                    Ok(format!("- {} [{}] ({}%)", title, status, progress))
                })?
                .filter_map(|r| r.ok())
                .collect();

            Ok::<_, anyhow::Error>(goals)
        })
        .await;

    result.unwrap_or_default()
}

/// Get memories relevant to the task
async fn get_relevant_memories(
    pool: &Arc<DatabasePool>,
    project_id: i64,
    task: &str,
) -> Vec<String> {
    let pool_clone = pool.clone();
    let task = task.to_string();

    // Extract keywords from task for matching
    let keywords: Vec<String> = task
        .split_whitespace()
        .filter(|w| w.len() > 3)
        .take(5)
        .map(|s| s.to_string())
        .collect();

    if keywords.is_empty() {
        return Vec::new();
    }

    let result = pool_clone
        .interact(move |conn| {
            // Build a simple keyword match query
            let like_clauses: Vec<String> = keywords
                .iter()
                .map(|_| "content LIKE '%' || ? || '%'".to_string())
                .collect();
            let where_clause = like_clauses.join(" OR ");

            let sql = format!(
                r#"
                SELECT content, fact_type
                FROM memory_facts
                WHERE project_id = ?
                  AND ({})
                ORDER BY
                    CASE fact_type
                        WHEN 'decision' THEN 1
                        WHEN 'preference' THEN 2
                        ELSE 3
                    END,
                    created_at DESC
                LIMIT 3
            "#,
                where_clause
            );

            let mut stmt = conn.prepare(&sql)?;

            // Build params: project_id + all keywords
            let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(project_id)];
            for kw in &keywords {
                params.push(Box::new(kw.clone()));
            }
            let params_refs: Vec<&dyn rusqlite::ToSql> =
                params.iter().map(|b| b.as_ref()).collect();

            let memories: Vec<String> = stmt
                .query_map(params_refs.as_slice(), |row| {
                    let content: String = row.get(0)?;
                    let fact_type: Option<String> = row.get(1)?;
                    let prefix = match fact_type.as_deref() {
                        Some("decision") => "[Decision]",
                        Some("preference") => "[Preference]",
                        _ => "[Context]",
                    };
                    // Truncate long content
                    let content = if content.len() > 150 {
                        format!("{}...", &content[..150])
                    } else {
                        content
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
