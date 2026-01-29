// crates/mira-server/src/db/tasks_tests.rs
// Tests for task and goal database operations

use super::pool::DatabasePool;
use super::{
    create_goal_sync, create_task_sync, delete_goal_sync, delete_task_sync, get_active_goals_sync,
    get_goal_by_id_sync, get_goals_sync, get_or_create_project_sync, get_pending_tasks_sync,
    get_task_by_id_sync, get_tasks_sync, update_goal_sync, update_task_sync,
};
use std::sync::Arc;

/// Helper to create a test pool with a project
async fn setup_test_pool() -> (Arc<DatabasePool>, i64) {
    let pool = Arc::new(
        DatabasePool::open_in_memory()
            .await
            .expect("Failed to open in-memory pool"),
    );
    let project_id = pool
        .interact(|conn| {
            get_or_create_project_sync(conn, "/test/path", Some("test")).map_err(Into::into)
        })
        .await
        .expect("Failed to create project")
        .0;
    (pool, project_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ═══════════════════════════════════════
    // create_task Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_create_task_basic() {
        let (pool, project_id) = setup_test_pool().await;

        let id = pool
            .interact(move |conn| {
                create_task_sync(
                    conn,
                    Some(project_id),
                    None,
                    "Test task",
                    Some("Test description"),
                    Some("pending"),
                    Some("high"),
                )
                .map_err(Into::into)
            })
            .await
            .unwrap();

        assert!(id > 0);
    }

    #[tokio::test]
    async fn test_create_task_with_defaults() {
        let (pool, project_id) = setup_test_pool().await;

        let id = pool
            .interact(move |conn| {
                create_task_sync(conn, Some(project_id), None, "Minimal task", None, None, None)
                    .map_err(Into::into)
            })
            .await
            .unwrap();

        assert!(id > 0);

        // Verify defaults
        let task = pool
            .interact(move |conn| get_task_by_id_sync(conn, id))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(task.title, "Minimal task");
        assert_eq!(task.status, "pending");
        assert_eq!(task.priority, "medium");
    }

    #[tokio::test]
    async fn test_create_task_with_goal() {
        let (pool, project_id) = setup_test_pool().await;

        // Create a goal first
        let goal_id = pool
            .interact(move |conn| {
                create_goal_sync(
                    conn,
                    Some(project_id),
                    "Test goal",
                    None,
                    Some("in_progress"),
                    Some("high"),
                    Some(50),
                )
                .map_err(Into::into)
            })
            .await
            .unwrap();

        let task_id = pool
            .interact(move |conn| {
                create_task_sync(
                    conn,
                    Some(project_id),
                    Some(goal_id),
                    "Task for goal",
                    None,
                    None,
                    None,
                )
                .map_err(Into::into)
            })
            .await
            .unwrap();

        let task = pool
            .interact(move |conn| get_task_by_id_sync(conn, task_id))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(task.goal_id, Some(goal_id));
    }

    #[tokio::test]
    async fn test_create_task_global() {
        let pool = Arc::new(DatabasePool::open_in_memory().await.unwrap());

        let id = pool
            .interact(|conn| {
                create_task_sync(conn, None, None, "Global task", None, None, None)
                    .map_err(Into::into)
            })
            .await
            .unwrap();

        assert!(id > 0);

        let task = pool
            .interact(move |conn| get_task_by_id_sync(conn, id))
            .await
            .unwrap()
            .unwrap();
        assert!(task.project_id.is_none());
    }

    // ═══════════════════════════════════════
    // get_task_by_id Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_get_task_by_id_existing() {
        let (pool, project_id) = setup_test_pool().await;

        let id = pool
            .interact(move |conn| {
                create_task_sync(
                    conn,
                    Some(project_id),
                    None,
                    "Find me",
                    Some("Description"),
                    None,
                    None,
                )
                .map_err(Into::into)
            })
            .await
            .unwrap();

        let task = pool
            .interact(move |conn| get_task_by_id_sync(conn, id))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(task.id, id);
        assert_eq!(task.title, "Find me");
        assert_eq!(task.description, Some("Description".to_string()));
    }

    #[tokio::test]
    async fn test_get_task_by_id_nonexistent() {
        let pool = Arc::new(DatabasePool::open_in_memory().await.unwrap());

        let task = pool
            .interact(|conn| get_task_by_id_sync(conn, 99999))
            .await
            .unwrap();
        assert!(task.is_none());
    }

    // ═══════════════════════════════════════
    // get_pending_tasks Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_get_pending_tasks_basic() {
        let (pool, project_id) = setup_test_pool().await;

        // Create pending tasks
        for i in 0..3 {
            let title = format!("Task {}", i);
            pool.interact(move |conn| {
                create_task_sync(
                    conn,
                    Some(project_id),
                    None,
                    &title,
                    None,
                    Some("pending"),
                    None,
                )
                .map_err(Into::into)
            })
            .await
            .unwrap();
        }

        // Create completed task
        pool.interact(move |conn| {
            create_task_sync(
                conn,
                Some(project_id),
                None,
                "Done task",
                None,
                Some("completed"),
                None,
            )
            .map_err(Into::into)
        })
        .await
        .unwrap();

        let pending = pool
            .interact(move |conn| get_pending_tasks_sync(conn, Some(project_id), 10))
            .await
            .unwrap();
        assert_eq!(pending.len(), 3);
        assert!(pending.iter().all(|t| t.status != "completed"));
    }

    #[tokio::test]
    async fn test_get_pending_tasks_limit() {
        let (pool, project_id) = setup_test_pool().await;

        for i in 0..10 {
            let title = format!("Task {}", i);
            pool.interact(move |conn| {
                create_task_sync(
                    conn,
                    Some(project_id),
                    None,
                    &title,
                    None,
                    Some("pending"),
                    None,
                )
                .map_err(Into::into)
            })
            .await
            .unwrap();
        }

        let pending = pool
            .interact(move |conn| get_pending_tasks_sync(conn, Some(project_id), 3))
            .await
            .unwrap();
        assert_eq!(pending.len(), 3);
    }

    #[tokio::test]
    async fn test_get_pending_tasks_global() {
        let pool = Arc::new(DatabasePool::open_in_memory().await.unwrap());

        pool.interact(|conn| {
            create_task_sync(conn, None, None, "Global pending", None, Some("pending"), None)
                .map_err(Into::into)
        })
        .await
        .unwrap();

        let pending = pool
            .interact(|conn| get_pending_tasks_sync(conn, None, 10))
            .await
            .unwrap();
        assert_eq!(pending.len(), 1);
        assert!(pending[0].project_id.is_none());
    }

    // ═══════════════════════════════════════
    // get_recent_tasks Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_get_recent_tasks_all_statuses() {
        let (pool, project_id) = setup_test_pool().await;

        pool.interact(move |conn| {
            create_task_sync(
                conn,
                Some(project_id),
                None,
                "Pending",
                None,
                Some("pending"),
                None,
            )
            .map_err(Into::into)
        })
        .await
        .unwrap();
        pool.interact(move |conn| {
            create_task_sync(
                conn,
                Some(project_id),
                None,
                "Completed",
                None,
                Some("completed"),
                None,
            )
            .map_err(Into::into)
        })
        .await
        .unwrap();

        let tasks = pool
            .interact(move |conn| get_tasks_sync(conn, Some(project_id), None).map_err(Into::into))
            .await
            .unwrap();
        assert_eq!(tasks.len(), 2);
    }

    #[tokio::test]
    async fn test_get_recent_tasks_ordering() {
        let (pool, project_id) = setup_test_pool().await;

        for i in 0..3 {
            let title = format!("Task {}", i);
            pool.interact(move |conn| {
                create_task_sync(conn, Some(project_id), None, &title, None, None, None)
                    .map_err(Into::into)
            })
            .await
            .unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }

        let tasks = pool
            .interact(move |conn| get_tasks_sync(conn, Some(project_id), None).map_err(Into::into))
            .await
            .unwrap();
        // Most recent first
        assert_eq!(tasks[0].title, "Task 2");
        assert_eq!(tasks[2].title, "Task 0");
    }

    // ═══════════════════════════════════════
    // get_tasks (with filter) Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_get_tasks_no_filter() {
        let (pool, project_id) = setup_test_pool().await;

        pool.interact(move |conn| {
            create_task_sync(
                conn,
                Some(project_id),
                None,
                "Task 1",
                None,
                Some("pending"),
                None,
            )
            .map_err(Into::into)
        })
        .await
        .unwrap();
        pool.interact(move |conn| {
            create_task_sync(
                conn,
                Some(project_id),
                None,
                "Task 2",
                None,
                Some("completed"),
                None,
            )
            .map_err(Into::into)
        })
        .await
        .unwrap();

        let tasks = pool
            .interact(move |conn| get_tasks_sync(conn, Some(project_id), None).map_err(Into::into))
            .await
            .unwrap();
        assert_eq!(tasks.len(), 2);
    }

    #[tokio::test]
    async fn test_get_tasks_with_status() {
        let (pool, project_id) = setup_test_pool().await;

        pool.interact(move |conn| {
            create_task_sync(
                conn,
                Some(project_id),
                None,
                "Pending",
                None,
                Some("pending"),
                None,
            )
            .map_err(Into::into)
        })
        .await
        .unwrap();
        pool.interact(move |conn| {
            create_task_sync(
                conn,
                Some(project_id),
                None,
                "In Progress",
                None,
                Some("in_progress"),
                None,
            )
            .map_err(Into::into)
        })
        .await
        .unwrap();

        let tasks = pool
            .interact(move |conn| {
                get_tasks_sync(conn, Some(project_id), Some("pending")).map_err(Into::into)
            })
            .await
            .unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].status, "pending");
    }

    #[tokio::test]
    async fn test_get_tasks_with_negation() {
        let (pool, project_id) = setup_test_pool().await;

        pool.interact(move |conn| {
            create_task_sync(
                conn,
                Some(project_id),
                None,
                "Pending",
                None,
                Some("pending"),
                None,
            )
            .map_err(Into::into)
        })
        .await
        .unwrap();
        pool.interact(move |conn| {
            create_task_sync(
                conn,
                Some(project_id),
                None,
                "Completed",
                None,
                Some("completed"),
                None,
            )
            .map_err(Into::into)
        })
        .await
        .unwrap();

        let tasks = pool
            .interact(move |conn| {
                get_tasks_sync(conn, Some(project_id), Some("!completed")).map_err(Into::into)
            })
            .await
            .unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].status, "pending");
    }

    // ═══════════════════════════════════════
    // update_task Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_update_task_title() {
        let (pool, project_id) = setup_test_pool().await;

        let id = pool
            .interact(move |conn| {
                create_task_sync(conn, Some(project_id), None, "Old title", None, None, None)
                    .map_err(Into::into)
            })
            .await
            .unwrap();

        pool.interact(move |conn| {
            update_task_sync(conn, id, Some("New title"), None, None).map_err(Into::into)
        })
        .await
        .unwrap();

        let task = pool
            .interact(move |conn| get_task_by_id_sync(conn, id))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(task.title, "New title");
    }

    #[tokio::test]
    async fn test_update_task_status() {
        let (pool, project_id) = setup_test_pool().await;

        let id = pool
            .interact(move |conn| {
                create_task_sync(
                    conn,
                    Some(project_id),
                    None,
                    "Task",
                    None,
                    Some("pending"),
                    None,
                )
                .map_err(Into::into)
            })
            .await
            .unwrap();

        pool.interact(move |conn| {
            update_task_sync(conn, id, None, Some("in_progress"), None).map_err(Into::into)
        })
        .await
        .unwrap();

        let task = pool
            .interact(move |conn| get_task_by_id_sync(conn, id))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(task.status, "in_progress");
    }

    #[tokio::test]
    async fn test_update_task_priority() {
        let (pool, project_id) = setup_test_pool().await;

        let id = pool
            .interact(move |conn| {
                create_task_sync(
                    conn,
                    Some(project_id),
                    None,
                    "Task",
                    None,
                    None,
                    Some("low"),
                )
                .map_err(Into::into)
            })
            .await
            .unwrap();

        pool.interact(move |conn| {
            update_task_sync(conn, id, None, None, Some("urgent")).map_err(Into::into)
        })
        .await
        .unwrap();

        let task = pool
            .interact(move |conn| get_task_by_id_sync(conn, id))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(task.priority, "urgent");
    }

    #[tokio::test]
    async fn test_update_task_multiple_fields() {
        let (pool, project_id) = setup_test_pool().await;

        let id = pool
            .interact(move |conn| {
                create_task_sync(
                    conn,
                    Some(project_id),
                    None,
                    "Old",
                    None,
                    Some("pending"),
                    Some("low"),
                )
                .map_err(Into::into)
            })
            .await
            .unwrap();

        pool.interact(move |conn| {
            update_task_sync(conn, id, Some("New title"), Some("in_progress"), Some("high"))
                .map_err(Into::into)
        })
        .await
        .unwrap();

        let task = pool
            .interact(move |conn| get_task_by_id_sync(conn, id))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(task.title, "New title");
        assert_eq!(task.status, "in_progress");
        assert_eq!(task.priority, "high");
    }

    #[tokio::test]
    async fn test_update_task_no_changes() {
        let (pool, project_id) = setup_test_pool().await;

        let id = pool
            .interact(move |conn| {
                create_task_sync(conn, Some(project_id), None, "Task", None, None, None)
                    .map_err(Into::into)
            })
            .await
            .unwrap();

        // Update with None for all fields should not error
        pool.interact(move |conn| {
            update_task_sync(conn, id, None, None, None).map_err(Into::into)
        })
        .await
        .unwrap();

        let task = pool
            .interact(move |conn| get_task_by_id_sync(conn, id))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(task.title, "Task");
    }

    // ═══════════════════════════════════════
    // delete_task Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_delete_task() {
        let (pool, project_id) = setup_test_pool().await;

        let id = pool
            .interact(move |conn| {
                create_task_sync(conn, Some(project_id), None, "To delete", None, None, None)
                    .map_err(Into::into)
            })
            .await
            .unwrap();

        pool.interact(move |conn| delete_task_sync(conn, id).map_err(Into::into))
            .await
            .unwrap();

        let task = pool
            .interact(move |conn| get_task_by_id_sync(conn, id))
            .await
            .unwrap();
        assert!(task.is_none());
    }

    #[tokio::test]
    async fn test_delete_nonexistent_task() {
        let pool = Arc::new(DatabasePool::open_in_memory().await.unwrap());

        // Deleting non-existent task should not error
        pool.interact(|conn| delete_task_sync(conn, 99999).map_err(Into::into))
            .await
            .unwrap();
    }

    // ═══════════════════════════════════════
    // create_goal Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_create_goal_basic() {
        let (pool, project_id) = setup_test_pool().await;

        let id = pool
            .interact(move |conn| {
                create_goal_sync(
                    conn,
                    Some(project_id),
                    "Test goal",
                    Some("Test description"),
                    Some("planning"),
                    Some("high"),
                    Some(0),
                )
                .map_err(Into::into)
            })
            .await
            .unwrap();

        assert!(id > 0);

        let goal = pool
            .interact(move |conn| get_goal_by_id_sync(conn, id))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(goal.title, "Test goal");
        assert_eq!(goal.description, Some("Test description".to_string()));
        assert_eq!(goal.status, "planning");
        assert_eq!(goal.priority, "high");
        assert_eq!(goal.progress_percent, 0);
    }

    #[tokio::test]
    async fn test_create_goal_with_defaults() {
        let (pool, project_id) = setup_test_pool().await;

        let id = pool
            .interact(move |conn| {
                create_goal_sync(conn, Some(project_id), "Minimal goal", None, None, None, None)
                    .map_err(Into::into)
            })
            .await
            .unwrap();

        let goal = pool
            .interact(move |conn| get_goal_by_id_sync(conn, id))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(goal.title, "Minimal goal");
        assert_eq!(goal.status, "planning");
        assert_eq!(goal.priority, "medium");
        assert_eq!(goal.progress_percent, 0);
    }

    #[tokio::test]
    async fn test_create_goal_global() {
        let pool = Arc::new(DatabasePool::open_in_memory().await.unwrap());

        let id = pool
            .interact(|conn| {
                create_goal_sync(conn, None, "Global goal", None, None, None, None)
                    .map_err(Into::into)
            })
            .await
            .unwrap();

        let goal = pool
            .interact(move |conn| get_goal_by_id_sync(conn, id))
            .await
            .unwrap()
            .unwrap();
        assert!(goal.project_id.is_none());
    }

    // ═══════════════════════════════════════
    // get_goal_by_id Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_get_goal_by_id_existing() {
        let (pool, project_id) = setup_test_pool().await;

        let id = pool
            .interact(move |conn| {
                create_goal_sync(
                    conn,
                    Some(project_id),
                    "Find me",
                    Some("Description"),
                    None,
                    None,
                    None,
                )
                .map_err(Into::into)
            })
            .await
            .unwrap();

        let goal = pool
            .interact(move |conn| get_goal_by_id_sync(conn, id))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(goal.id, id);
        assert_eq!(goal.title, "Find me");
    }

    #[tokio::test]
    async fn test_get_goal_by_id_nonexistent() {
        let pool = Arc::new(DatabasePool::open_in_memory().await.unwrap());

        let goal = pool
            .interact(|conn| get_goal_by_id_sync(conn, 99999))
            .await
            .unwrap();
        assert!(goal.is_none());
    }

    // ═══════════════════════════════════════
    // get_active_goals Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_get_active_goals_basic() {
        let (pool, project_id) = setup_test_pool().await;

        // Create active goals
        pool.interact(move |conn| {
            create_goal_sync(
                conn,
                Some(project_id),
                "In progress",
                None,
                Some("in_progress"),
                None,
                None,
            )
            .map_err(Into::into)
        })
        .await
        .unwrap();
        pool.interact(move |conn| {
            create_goal_sync(
                conn,
                Some(project_id),
                "Planning",
                None,
                Some("planning"),
                None,
                None,
            )
            .map_err(Into::into)
        })
        .await
        .unwrap();

        // Create completed goal
        pool.interact(move |conn| {
            create_goal_sync(
                conn,
                Some(project_id),
                "Done",
                None,
                Some("completed"),
                None,
                None,
            )
            .map_err(Into::into)
        })
        .await
        .unwrap();

        // Create abandoned goal
        pool.interact(move |conn| {
            create_goal_sync(
                conn,
                Some(project_id),
                "Abandoned",
                None,
                Some("abandoned"),
                None,
                None,
            )
            .map_err(Into::into)
        })
        .await
        .unwrap();

        let active = pool
            .interact(move |conn| get_active_goals_sync(conn, Some(project_id), 10))
            .await
            .unwrap();
        assert_eq!(active.len(), 2);
        assert!(
            active
                .iter()
                .all(|g| g.status != "completed" && g.status != "abandoned")
        );
    }

    #[tokio::test]
    async fn test_get_active_goals_limit() {
        let (pool, project_id) = setup_test_pool().await;

        for i in 0..5 {
            let title = format!("Goal {}", i);
            pool.interact(move |conn| {
                create_goal_sync(
                    conn,
                    Some(project_id),
                    &title,
                    None,
                    Some("in_progress"),
                    None,
                    None,
                )
                .map_err(Into::into)
            })
            .await
            .unwrap();
        }

        let active = pool
            .interact(move |conn| get_active_goals_sync(conn, Some(project_id), 3))
            .await
            .unwrap();
        assert_eq!(active.len(), 3);
    }

    // ═══════════════════════════════════════
    // get_goals (with filter) Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_get_goals_no_filter() {
        let (pool, project_id) = setup_test_pool().await;

        pool.interact(move |conn| {
            create_goal_sync(
                conn,
                Some(project_id),
                "Goal 1",
                None,
                Some("planning"),
                None,
                None,
            )
            .map_err(Into::into)
        })
        .await
        .unwrap();
        pool.interact(move |conn| {
            create_goal_sync(
                conn,
                Some(project_id),
                "Goal 2",
                None,
                Some("completed"),
                None,
                None,
            )
            .map_err(Into::into)
        })
        .await
        .unwrap();

        let goals = pool
            .interact(move |conn| get_goals_sync(conn, Some(project_id), None).map_err(Into::into))
            .await
            .unwrap();
        assert_eq!(goals.len(), 2);
    }

    #[tokio::test]
    async fn test_get_goals_with_status() {
        let (pool, project_id) = setup_test_pool().await;

        pool.interact(move |conn| {
            create_goal_sync(
                conn,
                Some(project_id),
                "Planning",
                None,
                Some("planning"),
                None,
                None,
            )
            .map_err(Into::into)
        })
        .await
        .unwrap();
        pool.interact(move |conn| {
            create_goal_sync(
                conn,
                Some(project_id),
                "In Progress",
                None,
                Some("in_progress"),
                None,
                None,
            )
            .map_err(Into::into)
        })
        .await
        .unwrap();

        let goals = pool
            .interact(move |conn| {
                get_goals_sync(conn, Some(project_id), Some("planning")).map_err(Into::into)
            })
            .await
            .unwrap();
        assert_eq!(goals.len(), 1);
        assert_eq!(goals[0].status, "planning");
    }

    #[tokio::test]
    async fn test_get_goals_with_negation() {
        let (pool, project_id) = setup_test_pool().await;

        pool.interact(move |conn| {
            create_goal_sync(
                conn,
                Some(project_id),
                "Active",
                None,
                Some("in_progress"),
                None,
                None,
            )
            .map_err(Into::into)
        })
        .await
        .unwrap();
        pool.interact(move |conn| {
            create_goal_sync(
                conn,
                Some(project_id),
                "Completed",
                None,
                Some("completed"),
                None,
                None,
            )
            .map_err(Into::into)
        })
        .await
        .unwrap();

        let goals = pool
            .interact(move |conn| {
                get_goals_sync(conn, Some(project_id), Some("!completed")).map_err(Into::into)
            })
            .await
            .unwrap();
        assert_eq!(goals.len(), 1);
        assert_eq!(goals[0].status, "in_progress");
    }

    // ═══════════════════════════════════════
    // update_goal Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_update_goal_title() {
        let (pool, project_id) = setup_test_pool().await;

        let id = pool
            .interact(move |conn| {
                create_goal_sync(conn, Some(project_id), "Old title", None, None, None, None)
                    .map_err(Into::into)
            })
            .await
            .unwrap();

        pool.interact(move |conn| {
            update_goal_sync(conn, id, Some("New title"), None, None, None).map_err(Into::into)
        })
        .await
        .unwrap();

        let goal = pool
            .interact(move |conn| get_goal_by_id_sync(conn, id))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(goal.title, "New title");
    }

    #[tokio::test]
    async fn test_update_goal_status() {
        let (pool, project_id) = setup_test_pool().await;

        let id = pool
            .interact(move |conn| {
                create_goal_sync(
                    conn,
                    Some(project_id),
                    "Goal",
                    None,
                    Some("planning"),
                    None,
                    None,
                )
                .map_err(Into::into)
            })
            .await
            .unwrap();

        pool.interact(move |conn| {
            update_goal_sync(conn, id, None, Some("in_progress"), None, None).map_err(Into::into)
        })
        .await
        .unwrap();

        let goal = pool
            .interact(move |conn| get_goal_by_id_sync(conn, id))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(goal.status, "in_progress");
    }

    #[tokio::test]
    async fn test_update_goal_priority() {
        let (pool, project_id) = setup_test_pool().await;

        let id = pool
            .interact(move |conn| {
                create_goal_sync(
                    conn,
                    Some(project_id),
                    "Goal",
                    None,
                    None,
                    Some("low"),
                    None,
                )
                .map_err(Into::into)
            })
            .await
            .unwrap();

        pool.interact(move |conn| {
            update_goal_sync(conn, id, None, None, Some("urgent"), None).map_err(Into::into)
        })
        .await
        .unwrap();

        let goal = pool
            .interact(move |conn| get_goal_by_id_sync(conn, id))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(goal.priority, "urgent");
    }

    #[tokio::test]
    async fn test_update_goal_progress() {
        let (pool, project_id) = setup_test_pool().await;

        let id = pool
            .interact(move |conn| {
                create_goal_sync(
                    conn,
                    Some(project_id),
                    "Goal",
                    None,
                    None,
                    None,
                    Some(25),
                )
                .map_err(Into::into)
            })
            .await
            .unwrap();

        pool.interact(move |conn| {
            update_goal_sync(conn, id, None, None, None, Some(75)).map_err(Into::into)
        })
        .await
        .unwrap();

        let goal = pool
            .interact(move |conn| get_goal_by_id_sync(conn, id))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(goal.progress_percent, 75);
    }

    #[tokio::test]
    async fn test_update_goal_multiple_fields() {
        let (pool, project_id) = setup_test_pool().await;

        let id = pool
            .interact(move |conn| {
                create_goal_sync(
                    conn,
                    Some(project_id),
                    "Old",
                    None,
                    Some("planning"),
                    Some("low"),
                    Some(0),
                )
                .map_err(Into::into)
            })
            .await
            .unwrap();

        pool.interact(move |conn| {
            update_goal_sync(
                conn,
                id,
                Some("New title"),
                Some("in_progress"),
                Some("high"),
                Some(50),
            )
            .map_err(Into::into)
        })
        .await
        .unwrap();

        let goal = pool
            .interact(move |conn| get_goal_by_id_sync(conn, id))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(goal.title, "New title");
        assert_eq!(goal.status, "in_progress");
        assert_eq!(goal.priority, "high");
        assert_eq!(goal.progress_percent, 50);
    }

    // ═══════════════════════════════════════
    // delete_goal Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_delete_goal() {
        let (pool, project_id) = setup_test_pool().await;

        let id = pool
            .interact(move |conn| {
                create_goal_sync(conn, Some(project_id), "To delete", None, None, None, None)
                    .map_err(Into::into)
            })
            .await
            .unwrap();

        pool.interact(move |conn| delete_goal_sync(conn, id).map_err(Into::into))
            .await
            .unwrap();

        let goal = pool
            .interact(move |conn| get_goal_by_id_sync(conn, id))
            .await
            .unwrap();
        assert!(goal.is_none());
    }

    #[tokio::test]
    async fn test_delete_nonexistent_goal() {
        let pool = Arc::new(DatabasePool::open_in_memory().await.unwrap());

        // Deleting non-existent goal should not error
        pool.interact(|conn| delete_goal_sync(conn, 99999).map_err(Into::into))
            .await
            .unwrap();
    }

    // ═══════════════════════════════════════
    // Task-Goal Relationship Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_task_goal_relationship() {
        let (pool, project_id) = setup_test_pool().await;

        let goal_id = pool
            .interact(move |conn| {
                create_goal_sync(conn, Some(project_id), "Parent goal", None, None, None, None)
                    .map_err(Into::into)
            })
            .await
            .unwrap();

        let task1_id = pool
            .interact(move |conn| {
                create_task_sync(
                    conn,
                    Some(project_id),
                    Some(goal_id),
                    "Subtask 1",
                    None,
                    None,
                    None,
                )
                .map_err(Into::into)
            })
            .await
            .unwrap();

        let task2_id = pool
            .interact(move |conn| {
                create_task_sync(
                    conn,
                    Some(project_id),
                    Some(goal_id),
                    "Subtask 2",
                    None,
                    None,
                    None,
                )
                .map_err(Into::into)
            })
            .await
            .unwrap();

        let task1 = pool
            .interact(move |conn| get_task_by_id_sync(conn, task1_id))
            .await
            .unwrap()
            .unwrap();
        let task2 = pool
            .interact(move |conn| get_task_by_id_sync(conn, task2_id))
            .await
            .unwrap()
            .unwrap();

        assert_eq!(task1.goal_id, Some(goal_id));
        assert_eq!(task2.goal_id, Some(goal_id));
    }

    #[tokio::test]
    async fn test_orphan_tasks() {
        let (pool, project_id) = setup_test_pool().await;

        // Create a task with a goal, then delete the goal
        let goal_id = pool
            .interact(move |conn| {
                create_goal_sync(
                    conn,
                    Some(project_id),
                    "Temporary goal",
                    None,
                    None,
                    None,
                    None,
                )
                .map_err(Into::into)
            })
            .await
            .unwrap();

        let task_id = pool
            .interact(move |conn| {
                create_task_sync(
                    conn,
                    Some(project_id),
                    Some(goal_id),
                    "Task",
                    None,
                    None,
                    None,
                )
                .map_err(Into::into)
            })
            .await
            .unwrap();

        pool.interact(move |conn| delete_goal_sync(conn, goal_id).map_err(Into::into))
            .await
            .unwrap();

        // Task should still exist
        let task = pool
            .interact(move |conn| get_task_by_id_sync(conn, task_id))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(task.title, "Task");
        // goal_id should be cleared (orphan task)
        assert_eq!(task.goal_id, None);
    }

    // ═══════════════════════════════════════
    // Project Isolation Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_task_project_isolation() {
        let (pool, project1) = setup_test_pool().await;
        let project2 = pool
            .interact(|conn| {
                get_or_create_project_sync(conn, "/other/path", Some("other")).map_err(Into::into)
            })
            .await
            .unwrap()
            .0;

        pool.interact(move |conn| {
            create_task_sync(
                conn,
                Some(project1),
                None,
                "Project 1 task",
                None,
                None,
                None,
            )
            .map_err(Into::into)
        })
        .await
        .unwrap();
        pool.interact(move |conn| {
            create_task_sync(
                conn,
                Some(project2),
                None,
                "Project 2 task",
                None,
                None,
                None,
            )
            .map_err(Into::into)
        })
        .await
        .unwrap();

        let tasks1 = pool
            .interact(move |conn| get_tasks_sync(conn, Some(project1), None).map_err(Into::into))
            .await
            .unwrap();
        let tasks2 = pool
            .interact(move |conn| get_tasks_sync(conn, Some(project2), None).map_err(Into::into))
            .await
            .unwrap();

        assert_eq!(tasks1.len(), 1);
        assert_eq!(tasks2.len(), 1);
        assert_eq!(tasks1[0].title, "Project 1 task");
        assert_eq!(tasks2[0].title, "Project 2 task");
    }

    #[tokio::test]
    async fn test_goal_project_isolation() {
        let (pool, project1) = setup_test_pool().await;
        let project2 = pool
            .interact(|conn| {
                get_or_create_project_sync(conn, "/other/path", Some("other")).map_err(Into::into)
            })
            .await
            .unwrap()
            .0;

        pool.interact(move |conn| {
            create_goal_sync(
                conn,
                Some(project1),
                "Project 1 goal",
                None,
                None,
                None,
                None,
            )
            .map_err(Into::into)
        })
        .await
        .unwrap();
        pool.interact(move |conn| {
            create_goal_sync(
                conn,
                Some(project2),
                "Project 2 goal",
                None,
                None,
                None,
                None,
            )
            .map_err(Into::into)
        })
        .await
        .unwrap();

        let goals1 = pool
            .interact(move |conn| get_goals_sync(conn, Some(project1), None).map_err(Into::into))
            .await
            .unwrap();
        let goals2 = pool
            .interact(move |conn| get_goals_sync(conn, Some(project2), None).map_err(Into::into))
            .await
            .unwrap();

        assert_eq!(goals1.len(), 1);
        assert_eq!(goals2.len(), 1);
        assert_eq!(goals1[0].title, "Project 1 goal");
        assert_eq!(goals2[0].title, "Project 2 goal");
    }

    // ═══════════════════════════════════════
    // Edge Cases
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_empty_title_task() {
        let (pool, project_id) = setup_test_pool().await;

        // Empty title should still work
        let id = pool
            .interact(move |conn| {
                create_task_sync(conn, Some(project_id), None, "", None, None, None)
                    .map_err(Into::into)
            })
            .await
            .unwrap();

        assert!(id > 0);
    }

    #[tokio::test]
    async fn test_empty_title_goal() {
        let (pool, project_id) = setup_test_pool().await;

        // Empty title should still work
        let id = pool
            .interact(move |conn| {
                create_goal_sync(conn, Some(project_id), "", None, None, None, None)
                    .map_err(Into::into)
            })
            .await
            .unwrap();

        assert!(id > 0);
    }

    #[tokio::test]
    async fn test_invalid_progress_percent() {
        let (pool, project_id) = setup_test_pool().await;

        // Should handle values outside 0-100 range
        let id = pool
            .interact(move |conn| {
                create_goal_sync(
                    conn,
                    Some(project_id),
                    "Goal",
                    None,
                    None,
                    None,
                    Some(150),
                )
                .map_err(Into::into)
            })
            .await
            .unwrap();

        let goal = pool
            .interact(move |conn| get_goal_by_id_sync(conn, id))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(goal.progress_percent, 150);
    }

    #[tokio::test]
    async fn test_negative_progress_percent() {
        let (pool, project_id) = setup_test_pool().await;

        let id = pool
            .interact(move |conn| {
                create_goal_sync(
                    conn,
                    Some(project_id),
                    "Goal",
                    None,
                    None,
                    None,
                    Some(-10),
                )
                .map_err(Into::into)
            })
            .await
            .unwrap();

        let goal = pool
            .interact(move |conn| get_goal_by_id_sync(conn, id))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(goal.progress_percent, -10);
    }
}
