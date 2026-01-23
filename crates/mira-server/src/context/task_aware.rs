// crates/mira-server/src/context/task_aware.rs
// Task-aware context injection

use crate::db::Database;
use std::sync::Arc;

pub struct TaskAwareInjector {
    db: Arc<Database>,
    project_id: Option<i64>,
}

impl TaskAwareInjector {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            db,
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
        match self.db.get_pending_tasks(self.project_id, 10) {
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
            if let Ok(Some(task)) = self.db.get_task_by_id(*id) {
                tasks.push(task);
            }
        }

        if tasks.is_empty() {
            return String::new();
        }

        // Also get active goals for broader context
        let goals = self
            .db
            .get_active_goals(self.project_id, 3)
            .unwrap_or_default();

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

    fn create_test_injector() -> TaskAwareInjector {
        let db = Arc::new(Database::open_in_memory().unwrap());
        TaskAwareInjector::new(db)
    }

    #[tokio::test]
    async fn test_empty_tasks() {
        let injector = create_test_injector();

        let ids = injector.get_active_task_ids().await;
        assert!(ids.is_empty());

        let context = injector.inject_task_context(vec![]).await;
        assert!(context.is_empty());
    }

    #[tokio::test]
    async fn test_with_tasks() {
        let db = Arc::new(Database::open_in_memory().unwrap());

        // Create a project first
        let project_id = db
            .get_or_create_project("/test/project", Some("test"))
            .unwrap()
            .0;

        // Create some tasks
        db.create_task(
            Some(project_id),
            None,
            "Fix the bug",
            Some("There's a bug in the login flow"),
            Some("pending"),
            Some("high"),
        )
        .unwrap();

        db.create_task(
            Some(project_id),
            None,
            "Add tests",
            None,
            Some("pending"),
            Some("medium"),
        )
        .unwrap();

        let mut injector = TaskAwareInjector::new(db);
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
        let db = Arc::new(Database::open_in_memory().unwrap());

        let project_id = db
            .get_or_create_project("/test/project", Some("test"))
            .unwrap()
            .0;

        // Create a goal
        db.create_goal(
            Some(project_id),
            "Launch v1.0",
            Some("First stable release"),
            Some("in_progress"),
            Some("high"),
            Some(50),
        )
        .unwrap();

        // Create a task
        let task_id = db
            .create_task(
                Some(project_id),
                None,
                "Write docs",
                None,
                Some("pending"),
                None,
            )
            .unwrap();

        let mut injector = TaskAwareInjector::new(db);
        injector.set_project_id(Some(project_id));

        let context = injector.inject_task_context(vec![task_id]).await;
        assert!(context.contains("Launch v1.0"));
        assert!(context.contains("50%"));
        assert!(context.contains("Write docs"));
    }
}
