// src/mcp/tools/tasks.rs
// Task and goal management tools

use crate::mcp::MiraServer;
use rusqlite::params;

/// Task management
pub async fn task(
    server: &MiraServer,
    action: String,
    task_id: Option<String>,
    title: Option<String>,
    description: Option<String>,
    status: Option<String>,
    priority: Option<String>,
    include_completed: Option<bool>,
    limit: Option<i64>,
) -> Result<String, String> {
    let project_id = server
        .project
        .read()
        .await
        .as_ref()
        .map(|p| p.id);

    let conn = server.db.conn();

    match action.as_str() {
        "create" => {
            let title = title.ok_or("Title required for create")?;
            let status = status.unwrap_or_else(|| "pending".to_string());
            let priority = priority.unwrap_or_else(|| "medium".to_string());

            conn.execute(
                "INSERT INTO tasks (project_id, title, description, status, priority) VALUES (?, ?, ?, ?, ?)",
                params![project_id, title, description, status, priority],
            ).map_err(|e| e.to_string())?;

            let id = conn.last_insert_rowid();
            Ok(format!("Created task {} (id: {})", title, id))
        }

        "list" => {
            let include_completed = include_completed.unwrap_or(false);
            let limit = limit.unwrap_or(20);

            let sql = if include_completed {
                "SELECT id, title, status, priority FROM tasks WHERE project_id = ? OR project_id IS NULL ORDER BY created_at DESC LIMIT ?"
            } else {
                "SELECT id, title, status, priority FROM tasks WHERE (project_id = ? OR project_id IS NULL) AND status != 'completed' ORDER BY created_at DESC LIMIT ?"
            };

            let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
            let tasks: Vec<(i64, String, String, String)> = stmt
                .query_map(params![project_id, limit], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
                })
                .map_err(|e| e.to_string())?
                .filter_map(|r| r.ok())
                .collect();

            if tasks.is_empty() {
                return Ok("No tasks found.".to_string());
            }

            let mut response = format!("{} tasks:\n", tasks.len());
            for (id, title, status, priority) in tasks {
                let icon = match status.as_str() {
                    "completed" => "✓",
                    "in_progress" => "→",
                    "blocked" => "✗",
                    _ => "○",
                };
                response.push_str(&format!("  {} [{}] {} ({})\n", icon, id, title, priority));
            }
            Ok(response)
        }

        "update" | "complete" => {
            let id: i64 = task_id
                .ok_or("Task ID required")?
                .parse()
                .map_err(|_| "Invalid task ID")?;

            let new_status = if action == "complete" {
                Some("completed".to_string())
            } else {
                status
            };

            if let Some(s) = new_status {
                conn.execute(
                    "UPDATE tasks SET status = ? WHERE id = ?",
                    params![s, id],
                ).map_err(|e| e.to_string())?;
            }

            if let Some(t) = title {
                conn.execute(
                    "UPDATE tasks SET title = ? WHERE id = ?",
                    params![t, id],
                ).map_err(|e| e.to_string())?;
            }

            if let Some(p) = priority {
                conn.execute(
                    "UPDATE tasks SET priority = ? WHERE id = ?",
                    params![p, id],
                ).map_err(|e| e.to_string())?;
            }

            Ok(format!("Updated task {}", id))
        }

        "delete" => {
            let id: i64 = task_id
                .ok_or("Task ID required")?
                .parse()
                .map_err(|_| "Invalid task ID")?;

            conn.execute("DELETE FROM tasks WHERE id = ?", [id])
                .map_err(|e| e.to_string())?;

            Ok(format!("Deleted task {}", id))
        }

        _ => Err(format!("Unknown action: {}. Use: create, list, update, complete, delete", action)),
    }
}

/// Goal management
pub async fn goal(
    server: &MiraServer,
    action: String,
    goal_id: Option<String>,
    title: Option<String>,
    description: Option<String>,
    status: Option<String>,
    priority: Option<String>,
    progress_percent: Option<i32>,
    include_finished: Option<bool>,
    limit: Option<i64>,
) -> Result<String, String> {
    let project_id = server
        .project
        .read()
        .await
        .as_ref()
        .map(|p| p.id);

    let conn = server.db.conn();

    match action.as_str() {
        "create" => {
            let title = title.ok_or("Title required for create")?;
            let status = status.unwrap_or_else(|| "planning".to_string());
            let priority = priority.unwrap_or_else(|| "medium".to_string());

            conn.execute(
                "INSERT INTO goals (project_id, title, description, status, priority) VALUES (?, ?, ?, ?, ?)",
                params![project_id, title, description, status, priority],
            ).map_err(|e| e.to_string())?;

            let id = conn.last_insert_rowid();
            Ok(format!("Created goal '{}' (id: {})", title, id))
        }

        "list" => {
            let include_finished = include_finished.unwrap_or(false);
            let limit = limit.unwrap_or(10);

            let sql = if include_finished {
                "SELECT id, title, status, priority, progress_percent FROM goals WHERE project_id = ? OR project_id IS NULL ORDER BY created_at DESC LIMIT ?"
            } else {
                "SELECT id, title, status, priority, progress_percent FROM goals WHERE (project_id = ? OR project_id IS NULL) AND status NOT IN ('completed', 'abandoned') ORDER BY created_at DESC LIMIT ?"
            };

            let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
            let goals: Vec<(i64, String, String, String, i32)> = stmt
                .query_map(params![project_id, limit], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?))
                })
                .map_err(|e| e.to_string())?
                .filter_map(|r| r.ok())
                .collect();

            if goals.is_empty() {
                return Ok("No goals found.".to_string());
            }

            let mut response = format!("{} goals:\n", goals.len());
            for (id, title, status, priority, progress) in goals {
                let icon = match status.as_str() {
                    "completed" => "✓",
                    "in_progress" => "→",
                    "abandoned" => "✗",
                    _ => "○",
                };
                response.push_str(&format!("  {} {} ({}%) - {} [{}]\n", icon, title, progress, priority, id));
            }
            Ok(response)
        }

        "update" | "progress" => {
            let id: i64 = goal_id
                .ok_or("Goal ID required")?
                .parse()
                .map_err(|_| "Invalid goal ID")?;

            if let Some(s) = status {
                conn.execute(
                    "UPDATE goals SET status = ? WHERE id = ?",
                    params![s, id],
                ).map_err(|e| e.to_string())?;
            }

            if let Some(p) = progress_percent {
                conn.execute(
                    "UPDATE goals SET progress_percent = ? WHERE id = ?",
                    params![p, id],
                ).map_err(|e| e.to_string())?;
            }

            if let Some(t) = title {
                conn.execute(
                    "UPDATE goals SET title = ? WHERE id = ?",
                    params![t, id],
                ).map_err(|e| e.to_string())?;
            }

            Ok(format!("Updated goal {}", id))
        }

        _ => Err(format!("Unknown action: {}. Use: create, list, update, progress", action)),
    }
}
