// crates/mira-server/src/mcp/tools/tasks.rs
// Task and goal management tools - delegates to unified core

use crate::mcp::MiraServer;
use crate::tools::core::tasks_goals;

/// Task management - delegates to unified core
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
    tasks: Option<String>,
) -> Result<String, String> {
    tasks_goals::task(
        server,
        action,
        task_id,
        title,
        description,
        status,
        priority,
        include_completed,
        limit,
        tasks,
    ).await
}

/// Goal management - delegates to unified core
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
    goals: Option<String>,
) -> Result<String, String> {
    tasks_goals::goal(
        server,
        action,
        goal_id,
        title,
        description,
        status,
        priority,
        progress_percent,
        include_finished,
        limit,
        goals,
    ).await
}
