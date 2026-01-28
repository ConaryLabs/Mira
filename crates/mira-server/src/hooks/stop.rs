// crates/mira-server/src/hooks/stop.rs
// Stop hook handler - checks goal progress and saves session state

use crate::db::pool::DatabasePool;
use crate::hooks::{read_hook_input, write_hook_output};
use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;

/// Get database path
fn get_db_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".mira/mira.db")
}

/// Stop hook input from Claude Code
#[derive(Debug)]
struct StopInput {
    session_id: String,
    stop_hook_active: bool,
}

impl StopInput {
    fn from_json(json: &serde_json::Value) -> Self {
        Self {
            session_id: json
                .get("session_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            stop_hook_active: json
                .get("stop_hook_active")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
        }
    }
}

/// Run Stop hook
///
/// This hook fires when Claude finishes responding. We can:
/// 1. Check if there are incomplete goals being worked on
/// 2. Save session state
/// 3. Optionally block stopping if goals need completion
pub async fn run() -> Result<()> {
    let input = read_hook_input()?;
    let stop_input = StopInput::from_json(&input);

    eprintln!(
        "[mira] Stop hook triggered (session: {}, stop_hook_active: {})",
        &stop_input.session_id[..stop_input.session_id.len().min(8)],
        stop_input.stop_hook_active
    );

    // Don't create infinite loops - if stop hook is already active, just allow stop
    if stop_input.stop_hook_active {
        eprintln!("[mira] Stop hook already active, allowing stop");
        write_hook_output(&serde_json::json!({}));
        return Ok(());
    }

    // Open database
    let db_path = get_db_path();
    let pool = Arc::new(DatabasePool::open(&db_path).await?);

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
        // No active project, just allow stop
        write_hook_output(&serde_json::json!({}));
        return Ok(());
    };

    // Check for in-progress goals
    let goals: Vec<GoalInfo> = {
        let pool_clone = pool.clone();
        pool_clone
            .interact(move |conn| Ok::<_, anyhow::Error>(get_in_progress_goals(conn, project_id)))
            .await
            .unwrap_or_default()
    };

    // Build output
    let mut output = serde_json::json!({});

    if !goals.is_empty() {
        // We have active goals - add context but don't block
        let goal_summary: Vec<String> = goals
            .iter()
            .map(|g| {
                format!(
                    "- {} ({}%): {}",
                    g.title,
                    g.progress,
                    g.next_milestone.as_deref().unwrap_or("no milestones")
                )
            })
            .collect();

        let context = format!("Active goals in progress:\n{}", goal_summary.join("\n"));

        // Add as additional context (informational, not blocking)
        output = serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "Stop",
                "additionalContext": context
            }
        });

        eprintln!("[mira] {} active goal(s) found", goals.len());
    }

    // Save session timestamp
    {
        let pool_clone = pool.clone();
        let session_id = stop_input.session_id.clone();
        let _ = pool_clone
            .interact(move |conn| {
                crate::db::set_server_state_sync(
                    conn,
                    "last_stop_time",
                    &chrono::Utc::now().to_rfc3339(),
                )
                .ok();
                if !session_id.is_empty() {
                    crate::db::set_server_state_sync(conn, "active_session_id", &session_id).ok();
                }
                Ok::<_, anyhow::Error>(())
            })
            .await;
    }

    write_hook_output(&output);
    Ok(())
}

/// Goal info for stop hook
struct GoalInfo {
    title: String,
    progress: i32,
    next_milestone: Option<String>,
}

/// Get in-progress goals for a project
fn get_in_progress_goals(conn: &rusqlite::Connection, project_id: i64) -> Vec<GoalInfo> {
    let sql = r#"
        SELECT g.title, g.progress_percent,
               (SELECT title FROM goal_milestones
                WHERE goal_id = g.id AND completed_at IS NULL
                ORDER BY id LIMIT 1) as next_milestone
        FROM goals g
        WHERE g.project_id = ? AND g.status = 'in_progress'
        ORDER BY g.priority DESC, g.created_at DESC
        LIMIT 5
    "#;

    let mut stmt = match conn.prepare(sql) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    stmt.query_map([project_id], |row| {
        Ok(GoalInfo {
            title: row.get(0)?,
            progress: row.get(1)?,
            next_milestone: row.get(2)?,
        })
    })
    .map(|rows| rows.filter_map(|r| r.ok()).collect())
    .unwrap_or_default()
}
