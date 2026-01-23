// crates/mira-server/src/context/task_aware.rs
// Task-aware context injection

use crate::db::pool::DatabasePool;
use crate::db::{get_pending_tasks_sync, get_task_by_id_sync, get_active_goals_sync};
use std::sync::Arc;

pub struct TaskAwareInjector {
    pool: Arc<DatabasePool>,
    project_id: Option<i64>,
}

impl TaskAwareInjector {
    pub fn new(pool: Arc<DatabasePool>) -> Self {
        Self {
            pool,
            project_id: None,
        }
    }

    /// Set the current project ID for task queries
    pub fn set_project_id(&mut self, project_id: Option<i64>) {
        self.project_id = project_id;
    }

    /// Get active task IDs for the current project
    /// Returns tasks with status 'pending' or 'in_progress'
    pub async fn get_active_task_ids(&self) -> Vec<i64> {
        let project_id = self.project_id;
        match self.pool.interact(move |conn| {
            get_pending_tasks_sync(conn, project_id, 10)
                .map_err(|e| anyhow::anyhow!("{}", e))
        }).await {
            Ok(tasks) => tasks.into_iter().map(|t| t.id).collect(),
            Err(e) => {
                tracing::debug!("Failed to get pending tasks: {}", e);
                Vec::new()
            }
        }
    }

    /// Inject context about active tasks
    /// Formats task information for context injection
    pub async fn inject_task_context(&self, task_ids: Vec<i64>) -> String {
        if task_ids.is_empty() {
            return String::new();
        }

        // Get full task details
        let mut tasks = Vec::new();
        for id in task_ids.iter().take(5) {
            // Limit to 5 tasks for context
            let task_id = *id;
            if let Ok(Some(task)) = self.pool.interact(move |conn| {
                get_task_by_id_sync(conn, task_id)
                    .map_err(|e| anyhow::anyhow!("{}", e))
            }).await {
                tasks.push(task);
            }
        }

        if tasks.is_empty() {
            return String::new();
        }

        // Also get active goals for broader context
        let project_id = self.project_id;
        let goals = self.pool.interact(move |conn| {
            get_active_goals_sync(conn, project_id, 3)
                .map_err(|e| anyhow::anyhow!("{}", e))
        }).await.unwrap_or_default();

        let mut context = String::new();

        // Add goals context if any
        if !goals.is_empty() {
            context.push_str("Active goals:\n");
            for goal in goals.iter().take(2) {
                context.push_str(&format!(
                    "  - {} ({}%, {})\n",
                    goal.title, goal.progress_percent, goal.status
                ));
            }
        }

        // Add tasks context
        context.push_str("Pending tasks:\n");
        for task in &tasks {
            let priority_marker = match task.priority.as_str() {
                "urgent" => "!!",
                "high" => "!",
                _ => "",
            };

            context.push_str(&format!("  - [{}] {}{}\n", task.id, priority_marker, task.title));

            // Add description snippet if present and short
            if let Some(ref desc) = task.description {
                if !desc.is_empty() && desc.len() <= 100 {
                    context.push_str(&format!("    {}\n", desc));
                }
            }
        }

        context.trim_end().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn create_test_injector() -> TaskAwareInjector {
        let pool = Arc::new(DatabasePool::open_in_memory().await.unwrap());
        TaskAwareInjector::new(pool)
    }

    #[tokio::test]
    async fn test_empty_tasks() {
        let injector = create_test_injector().await;

        let ids = injector.get_active_task_ids().await;
        assert!(ids.is_empty());

        let context = injector.inject_task_context(vec![]).await;
        assert!(context.is_empty());
    }

    #[tokio::test]
    async fn test_with_tasks() {
        let pool = Arc::new(DatabasePool::open_in_memory().await.unwrap());

        // Create a project first (via pool)
        let project_id = pool.interact(|conn| {
            crate::db::get_or_create_project_sync(conn, "/test/project", Some("test"))
                .map_err(|e| anyhow::anyhow!("{}", e))
        }).await.unwrap().0;

        // Create some tasks (need sync function or use pool)
        pool.interact(move |conn| {
            conn.execute(
                "INSERT INTO tasks (project_id, goal_id, title, description, status, priority) VALUES (?, ?, ?, ?, ?, ?)",
                rusqlite::params![project_id, Option::<i64>::None, "Fix the bug", Some("There's a bug in the login flow"), "pending", "high"],
            )?;
            conn.execute(
                "INSERT INTO tasks (project_id, goal_id, title, description, status, priority) VALUES (?, ?, ?, ?, ?, ?)",
                rusqlite::params![project_id, Option::<i64>::None, "Add tests", Option::<String>::None, "pending", "medium"],
            )?;
            Ok::<_, anyhow::Error>(())
        }).await.unwrap();

        let mut injector = TaskAwareInjector::new(pool);
        injector.set_project_id(Some(project_id));

        let ids = injector.get_active_task_ids().await;
        assert_eq!(ids.len(), 2);

        let context = injector.inject_task_context(ids).await;
        assert!(context.contains("Fix the bug"));
        assert!(context.contains("Add tests"));
        assert!(context.contains("!")); // high priority marker
    }

    #[tokio::test]
    async fn test_with_goals() {
        let pool = Arc::new(DatabasePool::open_in_memory().await.unwrap());

        let project_id = pool.interact(|conn| {
            crate::db::get_or_create_project_sync(conn, "/test/project", Some("test"))
                .map_err(|e| anyhow::anyhow!("{}", e))
        }).await.unwrap().0;

        // Create a goal and task via pool
        let task_id = pool.interact(move |conn| {
            conn.execute(
                "INSERT INTO goals (project_id, title, description, status, priority, progress_percent) VALUES (?, ?, ?, ?, ?, ?)",
                rusqlite::params![project_id, "Launch v1.0", Some("First stable release"), "in_progress", "high", 50],
            )?;
            conn.execute(
                "INSERT INTO tasks (project_id, goal_id, title, description, status, priority) VALUES (?, ?, ?, ?, ?, ?)",
                rusqlite::params![project_id, Option::<i64>::None, "Write docs", Option::<String>::None, "pending", "medium"],
            )?;
            Ok::<_, anyhow::Error>(conn.last_insert_rowid())
        }).await.unwrap();

        let mut injector = TaskAwareInjector::new(pool);
        injector.set_project_id(Some(project_id));

        let context = injector.inject_task_context(vec![task_id]).await;
        assert!(context.contains("Launch v1.0"));
        assert!(context.contains("50%"));
        assert!(context.contains("Write docs"));
    }
}
