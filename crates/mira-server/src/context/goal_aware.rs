// crates/mira-server/src/context/goal_aware.rs
// Goal-aware context injection

use crate::db::pool::DatabasePool;
use crate::db::{get_active_goals_sync, get_milestones_for_goal_sync};
use std::sync::Arc;

pub struct GoalAwareInjector {
    pool: Arc<DatabasePool>,
    project_id: Option<i64>,
}

impl GoalAwareInjector {
    pub fn new(pool: Arc<DatabasePool>) -> Self {
        Self {
            pool,
            project_id: None,
        }
    }

    /// Set the current project ID for goal queries
    pub fn set_project_id(&mut self, project_id: Option<i64>) {
        self.project_id = project_id;
    }

    /// Get active goal IDs for the current project
    /// Returns goals with status not in 'completed' or 'abandoned'
    pub async fn get_active_goal_ids(&self) -> Vec<i64> {
        let project_id = self.project_id;
        match self.pool.interact(move |conn| {
            get_active_goals_sync(conn, project_id, 10)
                .map_err(|e| anyhow::anyhow!("{}", e))
        }).await {
            Ok(goals) => goals.into_iter().map(|g| g.id).collect(),
            Err(e) => {
                tracing::debug!("Failed to get active goals: {}", e);
                Vec::new()
            }
        }
    }

    // Legacy method name for compatibility with context injection manager
    pub async fn get_active_task_ids(&self) -> Vec<i64> {
        self.get_active_goal_ids().await
    }

    /// Inject context about active goals and their milestones
    pub async fn inject_goal_context(&self, goal_ids: Vec<i64>) -> String {
        if goal_ids.is_empty() {
            return String::new();
        }

        let project_id = self.project_id;
        let goals = match self.pool.interact(move |conn| {
            get_active_goals_sync(conn, project_id, 5)
                .map_err(|e| anyhow::anyhow!("{}", e))
        }).await {
            Ok(goals) => goals,
            Err(_) => return String::new(),
        };

        if goals.is_empty() {
            return String::new();
        }

        let mut context = String::new();
        context.push_str("Active goals:\n");

        for goal in goals.iter().take(5) {
            // Get milestones for this goal
            let gid = goal.id;
            let milestones = self.pool.interact(move |conn| {
                get_milestones_for_goal_sync(conn, gid)
                    .map_err(|e| anyhow::anyhow!("{}", e))
            }).await.unwrap_or_default();

            let milestone_summary = if milestones.is_empty() {
                String::new()
            } else {
                let completed = milestones.iter().filter(|m| m.completed).count();
                let total = milestones.len();
                format!(" - {}/{} milestones", completed, total)
            };

            context.push_str(&format!(
                "  - Goal: {} ({}%){}\n",
                goal.title, goal.progress_percent, milestone_summary
            ));
        }

        context.trim_end().to_string()
    }

    // Legacy method name for compatibility with context injection manager
    pub async fn inject_task_context(&self, _task_ids: Vec<i64>) -> String {
        // Now injects goal context instead
        let goal_ids = self.get_active_goal_ids().await;
        self.inject_goal_context(goal_ids).await
    }
}

// Export as TaskAwareInjector for backwards compatibility
pub type TaskAwareInjector = GoalAwareInjector;

#[cfg(test)]
mod tests {
    use super::*;

    async fn create_test_injector() -> GoalAwareInjector {
        let pool = Arc::new(DatabasePool::open_in_memory().await.unwrap());
        GoalAwareInjector::new(pool)
    }

    #[tokio::test]
    async fn test_empty_goals() {
        let injector = create_test_injector().await;

        let ids = injector.get_active_goal_ids().await;
        assert!(ids.is_empty());

        let context = injector.inject_goal_context(vec![]).await;
        assert!(context.is_empty());
    }

    #[tokio::test]
    async fn test_with_goals() {
        let pool = Arc::new(DatabasePool::open_in_memory().await.unwrap());

        // Create a project first (via pool)
        let project_id = pool.interact(|conn| {
            crate::db::get_or_create_project_sync(conn, "/test/project", Some("test"))
                .map_err(|e| anyhow::anyhow!("{}", e))
        }).await.unwrap().0;

        // Create some goals
        pool.interact(move |conn| {
            conn.execute(
                "INSERT INTO goals (project_id, title, description, status, priority, progress_percent) VALUES (?, ?, ?, ?, ?, ?)",
                rusqlite::params![project_id, "Launch v1.0", Some("First stable release"), "in_progress", "high", 50],
            )?;
            conn.execute(
                "INSERT INTO goals (project_id, title, description, status, priority, progress_percent) VALUES (?, ?, ?, ?, ?, ?)",
                rusqlite::params![project_id, "Add documentation", Option::<String>::None, "planning", "medium", 0],
            )?;
            Ok::<_, anyhow::Error>(())
        }).await.unwrap();

        let mut injector = GoalAwareInjector::new(pool);
        injector.set_project_id(Some(project_id));

        let ids = injector.get_active_goal_ids().await;
        assert_eq!(ids.len(), 2);

        let context = injector.inject_goal_context(ids).await;
        assert!(context.contains("Launch v1.0"));
        assert!(context.contains("50%"));
        assert!(context.contains("Add documentation"));
    }

    #[tokio::test]
    async fn test_with_milestones() {
        let pool = Arc::new(DatabasePool::open_in_memory().await.unwrap());

        let project_id = pool.interact(|conn| {
            crate::db::get_or_create_project_sync(conn, "/test/project", Some("test"))
                .map_err(|e| anyhow::anyhow!("{}", e))
        }).await.unwrap().0;

        // Create a goal with milestones
        pool.interact(move |conn| {
            conn.execute(
                "INSERT INTO goals (project_id, title, description, status, priority, progress_percent) VALUES (?, ?, ?, ?, ?, ?)",
                rusqlite::params![project_id, "Feature X", Some("New feature"), "in_progress", "high", 33],
            )?;
            let goal_id = conn.last_insert_rowid();
            // Add milestones
            conn.execute(
                "INSERT INTO milestones (goal_id, title, completed, weight) VALUES (?, ?, ?, ?)",
                rusqlite::params![goal_id, "Design", 1, 1],
            )?;
            conn.execute(
                "INSERT INTO milestones (goal_id, title, completed, weight) VALUES (?, ?, ?, ?)",
                rusqlite::params![goal_id, "Implement", 0, 2],
            )?;
            Ok::<_, anyhow::Error>(())
        }).await.unwrap();

        let mut injector = GoalAwareInjector::new(pool);
        injector.set_project_id(Some(project_id));

        let context = injector.inject_goal_context(vec![1]).await;
        assert!(context.contains("Feature X"));
        assert!(context.contains("1/2 milestones"));
    }
}
