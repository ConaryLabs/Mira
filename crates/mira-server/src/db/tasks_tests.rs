// crates/mira-server/src/db/tasks_tests.rs
// Tests for task and goal database operations

use super::*;

/// Helper to create a test database with a project
fn setup_test_db() -> (Database, i64) {
    let db = Database::open_in_memory().expect("Failed to open in-memory db");
    let (project_id, _) = db.get_or_create_project("/test/path", Some("test")).unwrap();
    (db, project_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ═══════════════════════════════════════
    // create_task Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_create_task_basic() {
        let (db, project_id) = setup_test_db();

        let id = db
            .create_task(
                Some(project_id),
                None,
                "Test task",
                Some("Test description"),
                Some("pending"),
                Some("high"),
            )
            .unwrap();

        assert!(id > 0);
    }

    #[test]
    fn test_create_task_with_defaults() {
        let (db, project_id) = setup_test_db();

        let id = db
            .create_task(Some(project_id), None, "Minimal task", None, None, None)
            .unwrap();

        assert!(id > 0);

        // Verify defaults
        let task = db.get_task_by_id(id).unwrap().unwrap();
        assert_eq!(task.title, "Minimal task");
        assert_eq!(task.status, "pending");
        assert_eq!(task.priority, "medium");
    }

    #[test]
    fn test_create_task_with_goal() {
        let (db, project_id) = setup_test_db();

        // Create a goal first
        let goal_id = db
            .create_goal(
                Some(project_id),
                "Test goal",
                None,
                Some("in_progress"),
                Some("high"),
                Some(50),
            )
            .unwrap();

        let task_id = db
            .create_task(
                Some(project_id),
                Some(goal_id),
                "Task for goal",
                None,
                None,
                None,
            )
            .unwrap();

        let task = db.get_task_by_id(task_id).unwrap().unwrap();
        assert_eq!(task.goal_id, Some(goal_id));
    }

    #[test]
    fn test_create_task_global() {
        let db = Database::open_in_memory().unwrap();

        let id = db.create_task(None, None, "Global task", None, None, None).unwrap();

        assert!(id > 0);

        let task = db.get_task_by_id(id).unwrap().unwrap();
        assert!(task.project_id.is_none());
    }

    // ═══════════════════════════════════════
    // get_task_by_id Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_get_task_by_id_existing() {
        let (db, project_id) = setup_test_db();

        let id = db
            .create_task(
                Some(project_id),
                None,
                "Find me",
                Some("Description"),
                None,
                None,
            )
            .unwrap();

        let task = db.get_task_by_id(id).unwrap().unwrap();
        assert_eq!(task.id, id);
        assert_eq!(task.title, "Find me");
        assert_eq!(task.description, Some("Description".to_string()));
    }

    #[test]
    fn test_get_task_by_id_nonexistent() {
        let db = Database::open_in_memory().unwrap();

        let task = db.get_task_by_id(99999).unwrap();
        assert!(task.is_none());
    }

    // ═══════════════════════════════════════
    // get_pending_tasks Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_get_pending_tasks_basic() {
        let (db, project_id) = setup_test_db();

        // Create pending tasks
        for i in 0..3 {
            db.create_task(
                Some(project_id),
                None,
                &format!("Task {}", i),
                None,
                Some("pending"),
                None,
            )
            .unwrap();
        }

        // Create completed task
        db.create_task(
            Some(project_id),
            None,
            "Done task",
            None,
            Some("completed"),
            None,
        )
        .unwrap();

        let pending = db.get_pending_tasks(Some(project_id), 10).unwrap();
        assert_eq!(pending.len(), 3);
        assert!(pending.iter().all(|t| t.status != "completed"));
    }

    #[test]
    fn test_get_pending_tasks_limit() {
        let (db, project_id) = setup_test_db();

        for i in 0..10 {
            db.create_task(
                Some(project_id),
                None,
                &format!("Task {}", i),
                None,
                Some("pending"),
                None,
            )
            .unwrap();
        }

        let pending = db.get_pending_tasks(Some(project_id), 3).unwrap();
        assert_eq!(pending.len(), 3);
    }

    #[test]
    fn test_get_pending_tasks_global() {
        let db = Database::open_in_memory().unwrap();

        db.create_task(None, None, "Global pending", None, Some("pending"), None)
            .unwrap();

        let pending = db.get_pending_tasks(None, 10).unwrap();
        assert_eq!(pending.len(), 1);
        assert!(pending[0].project_id.is_none());
    }

    // ═══════════════════════════════════════
    // get_recent_tasks Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_get_recent_tasks_all_statuses() {
        let (db, project_id) = setup_test_db();

        db.create_task(
            Some(project_id),
            None,
            "Pending",
            None,
            Some("pending"),
            None,
        )
        .unwrap();
        db.create_task(
            Some(project_id),
            None,
            "Completed",
            None,
            Some("completed"),
            None,
        )
        .unwrap();

        let tasks = db.get_recent_tasks(Some(project_id), 10).unwrap();
        assert_eq!(tasks.len(), 2);
    }

    #[test]
    fn test_get_recent_tasks_ordering() {
        let (db, project_id) = setup_test_db();

        for i in 0..3 {
            db.create_task(
                Some(project_id),
                None,
                &format!("Task {}", i),
                None,
                None,
                None,
            )
            .unwrap();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }

        let tasks = db.get_recent_tasks(Some(project_id), 10).unwrap();
        // Most recent first
        assert_eq!(tasks[0].title, "Task 2");
        assert_eq!(tasks[2].title, "Task 0");
    }

    // ═══════════════════════════════════════
    // get_tasks (with filter) Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_get_tasks_no_filter() {
        let (db, project_id) = setup_test_db();

        db.create_task(
            Some(project_id),
            None,
            "Task 1",
            None,
            Some("pending"),
            None,
        )
        .unwrap();
        db.create_task(
            Some(project_id),
            None,
            "Task 2",
            None,
            Some("completed"),
            None,
        )
        .unwrap();

        let tasks = db.get_tasks(Some(project_id), None).unwrap();
        assert_eq!(tasks.len(), 2);
    }

    #[test]
    fn test_get_tasks_with_status() {
        let (db, project_id) = setup_test_db();

        db.create_task(
            Some(project_id),
            None,
            "Pending",
            None,
            Some("pending"),
            None,
        )
        .unwrap();
        db.create_task(
            Some(project_id),
            None,
            "In Progress",
            None,
            Some("in_progress"),
            None,
        )
        .unwrap();

        let tasks = db.get_tasks(Some(project_id), Some("pending")).unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].status, "pending");
    }

    #[test]
    fn test_get_tasks_with_negation() {
        let (db, project_id) = setup_test_db();

        db.create_task(
            Some(project_id),
            None,
            "Pending",
            None,
            Some("pending"),
            None,
        )
        .unwrap();
        db.create_task(
            Some(project_id),
            None,
            "Completed",
            None,
            Some("completed"),
            None,
        )
        .unwrap();

        let tasks = db.get_tasks(Some(project_id), Some("!completed")).unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].status, "pending");
    }

    // ═══════════════════════════════════════
    // update_task Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_update_task_title() {
        let (db, project_id) = setup_test_db();

        let id = db
            .create_task(
                Some(project_id),
                None,
                "Old title",
                None,
                None,
                None,
            )
            .unwrap();

        db.update_task(id, Some("New title"), None, None).unwrap();

        let task = db.get_task_by_id(id).unwrap().unwrap();
        assert_eq!(task.title, "New title");
    }

    #[test]
    fn test_update_task_status() {
        let (db, project_id) = setup_test_db();

        let id = db
            .create_task(
                Some(project_id),
                None,
                "Task",
                None,
                Some("pending"),
                None,
            )
            .unwrap();

        db.update_task(id, None, Some("in_progress"), None).unwrap();

        let task = db.get_task_by_id(id).unwrap().unwrap();
        assert_eq!(task.status, "in_progress");
    }

    #[test]
    fn test_update_task_priority() {
        let (db, project_id) = setup_test_db();

        let id = db
            .create_task(
                Some(project_id),
                None,
                "Task",
                None,
                None,
                Some("low"),
            )
            .unwrap();

        db.update_task(id, None, None, Some("urgent")).unwrap();

        let task = db.get_task_by_id(id).unwrap().unwrap();
        assert_eq!(task.priority, "urgent");
    }

    #[test]
    fn test_update_task_multiple_fields() {
        let (db, project_id) = setup_test_db();

        let id = db
            .create_task(
                Some(project_id),
                None,
                "Old",
                None,
                Some("pending"),
                Some("low"),
            )
            .unwrap();

        db.update_task(
            id,
            Some("New title"),
            Some("in_progress"),
            Some("high"),
        )
        .unwrap();

        let task = db.get_task_by_id(id).unwrap().unwrap();
        assert_eq!(task.title, "New title");
        assert_eq!(task.status, "in_progress");
        assert_eq!(task.priority, "high");
    }

    #[test]
    fn test_update_task_no_changes() {
        let (db, project_id) = setup_test_db();

        let id = db
            .create_task(
                Some(project_id),
                None,
                "Task",
                None,
                None,
                None,
            )
            .unwrap();

        // Update with None for all fields should not error
        db.update_task(id, None, None, None).unwrap();

        let task = db.get_task_by_id(id).unwrap().unwrap();
        assert_eq!(task.title, "Task");
    }

    // ═══════════════════════════════════════
    // delete_task Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_delete_task() {
        let (db, project_id) = setup_test_db();

        let id = db
            .create_task(
                Some(project_id),
                None,
                "To delete",
                None,
                None,
                None,
            )
            .unwrap();

        db.delete_task(id).unwrap();

        let task = db.get_task_by_id(id).unwrap();
        assert!(task.is_none());
    }

    #[test]
    fn test_delete_nonexistent_task() {
        let db = Database::open_in_memory().unwrap();

        // Deleting non-existent task should not error
        db.delete_task(99999).unwrap();
    }

    // ═══════════════════════════════════════
    // create_goal Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_create_goal_basic() {
        let (db, project_id) = setup_test_db();

        let id = db
            .create_goal(
                Some(project_id),
                "Test goal",
                Some("Test description"),
                Some("planning"),
                Some("high"),
                Some(0),
            )
            .unwrap();

        assert!(id > 0);

        let goal = db.get_goal_by_id(id).unwrap().unwrap();
        assert_eq!(goal.title, "Test goal");
        assert_eq!(goal.description, Some("Test description".to_string()));
        assert_eq!(goal.status, "planning");
        assert_eq!(goal.priority, "high");
        assert_eq!(goal.progress_percent, 0);
    }

    #[test]
    fn test_create_goal_with_defaults() {
        let (db, project_id) = setup_test_db();

        let id = db
            .create_goal(Some(project_id), "Minimal goal", None, None, None, None)
            .unwrap();

        let goal = db.get_goal_by_id(id).unwrap().unwrap();
        assert_eq!(goal.title, "Minimal goal");
        assert_eq!(goal.status, "planning");
        assert_eq!(goal.priority, "medium");
        assert_eq!(goal.progress_percent, 0);
    }

    #[test]
    fn test_create_goal_global() {
        let db = Database::open_in_memory().unwrap();

        let id = db.create_goal(None, "Global goal", None, None, None, None).unwrap();

        let goal = db.get_goal_by_id(id).unwrap().unwrap();
        assert!(goal.project_id.is_none());
    }

    // ═══════════════════════════════════════
    // get_goal_by_id Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_get_goal_by_id_existing() {
        let (db, project_id) = setup_test_db();

        let id = db
            .create_goal(
                Some(project_id),
                "Find me",
                Some("Description"),
                None,
                None,
                None,
            )
            .unwrap();

        let goal = db.get_goal_by_id(id).unwrap().unwrap();
        assert_eq!(goal.id, id);
        assert_eq!(goal.title, "Find me");
    }

    #[test]
    fn test_get_goal_by_id_nonexistent() {
        let db = Database::open_in_memory().unwrap();

        let goal = db.get_goal_by_id(99999).unwrap();
        assert!(goal.is_none());
    }

    // ═══════════════════════════════════════
    // get_active_goals Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_get_active_goals_basic() {
        let (db, project_id) = setup_test_db();

        // Create active goals
        db.create_goal(
            Some(project_id),
            "In progress",
            None,
            Some("in_progress"),
            None,
            None,
        )
        .unwrap();
        db.create_goal(
            Some(project_id),
            "Planning",
            None,
            Some("planning"),
            None,
            None,
        )
        .unwrap();

        // Create completed goal
        db.create_goal(
            Some(project_id),
            "Done",
            None,
            Some("completed"),
            None,
            None,
        )
        .unwrap();

        // Create abandoned goal
        db.create_goal(
            Some(project_id),
            "Abandoned",
            None,
            Some("abandoned"),
            None,
            None,
        )
        .unwrap();

        let active = db.get_active_goals(Some(project_id), 10).unwrap();
        assert_eq!(active.len(), 2);
        assert!(active.iter().all(|g| g.status != "completed" && g.status != "abandoned"));
    }

    #[test]
    fn test_get_active_goals_limit() {
        let (db, project_id) = setup_test_db();

        for i in 0..5 {
            db.create_goal(
                Some(project_id),
                &format!("Goal {}", i),
                None,
                Some("in_progress"),
                None,
                None,
            )
            .unwrap();
        }

        let active = db.get_active_goals(Some(project_id), 3).unwrap();
        assert_eq!(active.len(), 3);
    }

    // ═══════════════════════════════════════
    // get_goals (with filter) Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_get_goals_no_filter() {
        let (db, project_id) = setup_test_db();

        db.create_goal(
            Some(project_id),
            "Goal 1",
            None,
            Some("planning"),
            None,
            None,
        )
        .unwrap();
        db.create_goal(
            Some(project_id),
            "Goal 2",
            None,
            Some("completed"),
            None,
            None,
        )
        .unwrap();

        let goals = db.get_goals(Some(project_id), None).unwrap();
        assert_eq!(goals.len(), 2);
    }

    #[test]
    fn test_get_goals_with_status() {
        let (db, project_id) = setup_test_db();

        db.create_goal(
            Some(project_id),
            "Planning",
            None,
            Some("planning"),
            None,
            None,
        )
        .unwrap();
        db.create_goal(
            Some(project_id),
            "In Progress",
            None,
            Some("in_progress"),
            None,
            None,
        )
        .unwrap();

        let goals = db.get_goals(Some(project_id), Some("planning")).unwrap();
        assert_eq!(goals.len(), 1);
        assert_eq!(goals[0].status, "planning");
    }

    #[test]
    fn test_get_goals_with_negation() {
        let (db, project_id) = setup_test_db();

        db.create_goal(
            Some(project_id),
            "Active",
            None,
            Some("in_progress"),
            None,
            None,
        )
        .unwrap();
        db.create_goal(
            Some(project_id),
            "Completed",
            None,
            Some("completed"),
            None,
            None,
        )
        .unwrap();

        let goals = db.get_goals(Some(project_id), Some("!completed")).unwrap();
        assert_eq!(goals.len(), 1);
        assert_eq!(goals[0].status, "in_progress");
    }

    // ═══════════════════════════════════════
    // update_goal Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_update_goal_title() {
        let (db, project_id) = setup_test_db();

        let id = db
            .create_goal(
                Some(project_id),
                "Old title",
                None,
                None,
                None,
                None,
            )
            .unwrap();

        db.update_goal(id, Some("New title"), None, None, None).unwrap();

        let goal = db.get_goal_by_id(id).unwrap().unwrap();
        assert_eq!(goal.title, "New title");
    }

    #[test]
    fn test_update_goal_status() {
        let (db, project_id) = setup_test_db();

        let id = db
            .create_goal(
                Some(project_id),
                "Goal",
                None,
                Some("planning"),
                None,
                None,
            )
            .unwrap();

        db.update_goal(id, None, Some("in_progress"), None, None).unwrap();

        let goal = db.get_goal_by_id(id).unwrap().unwrap();
        assert_eq!(goal.status, "in_progress");
    }

    #[test]
    fn test_update_goal_priority() {
        let (db, project_id) = setup_test_db();

        let id = db
            .create_goal(
                Some(project_id),
                "Goal",
                None,
                None,
                Some("low"),
                None,
            )
            .unwrap();

        db.update_goal(id, None, None, Some("urgent"), None).unwrap();

        let goal = db.get_goal_by_id(id).unwrap().unwrap();
        assert_eq!(goal.priority, "urgent");
    }

    #[test]
    fn test_update_goal_progress() {
        let (db, project_id) = setup_test_db();

        let id = db
            .create_goal(
                Some(project_id),
                "Goal",
                None,
                None,
                None,
                Some(25),
            )
            .unwrap();

        db.update_goal(id, None, None, None, Some(75)).unwrap();

        let goal = db.get_goal_by_id(id).unwrap().unwrap();
        assert_eq!(goal.progress_percent, 75);
    }

    #[test]
    fn test_update_goal_multiple_fields() {
        let (db, project_id) = setup_test_db();

        let id = db
            .create_goal(
                Some(project_id),
                "Old",
                None,
                Some("planning"),
                Some("low"),
                Some(0),
            )
            .unwrap();

        db.update_goal(
            id,
            Some("New title"),
            Some("in_progress"),
            Some("high"),
            Some(50),
        )
        .unwrap();

        let goal = db.get_goal_by_id(id).unwrap().unwrap();
        assert_eq!(goal.title, "New title");
        assert_eq!(goal.status, "in_progress");
        assert_eq!(goal.priority, "high");
        assert_eq!(goal.progress_percent, 50);
    }

    // ═══════════════════════════════════════
    // delete_goal Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_delete_goal() {
        let (db, project_id) = setup_test_db();

        let id = db
            .create_goal(
                Some(project_id),
                "To delete",
                None,
                None,
                None,
                None,
            )
            .unwrap();

        db.delete_goal(id).unwrap();

        let goal = db.get_goal_by_id(id).unwrap();
        assert!(goal.is_none());
    }

    #[test]
    fn test_delete_nonexistent_goal() {
        let db = Database::open_in_memory().unwrap();

        // Deleting non-existent goal should not error
        db.delete_goal(99999).unwrap();
    }

    // ═══════════════════════════════════════
    // Task-Goal Relationship Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_task_goal_relationship() {
        let (db, project_id) = setup_test_db();

        let goal_id = db
            .create_goal(
                Some(project_id),
                "Parent goal",
                None,
                None,
                None,
                None,
            )
            .unwrap();

        let task1_id = db
            .create_task(
                Some(project_id),
                Some(goal_id),
                "Subtask 1",
                None,
                None,
                None,
            )
            .unwrap();

        let task2_id = db
            .create_task(
                Some(project_id),
                Some(goal_id),
                "Subtask 2",
                None,
                None,
                None,
            )
            .unwrap();

        let task1 = db.get_task_by_id(task1_id).unwrap().unwrap();
        let task2 = db.get_task_by_id(task2_id).unwrap().unwrap();

        assert_eq!(task1.goal_id, Some(goal_id));
        assert_eq!(task2.goal_id, Some(goal_id));
    }

    #[test]
    fn test_orphan_tasks() {
        let (db, project_id) = setup_test_db();

        // Create a task with a goal, then delete the goal
        let goal_id = db
            .create_goal(
                Some(project_id),
                "Temporary goal",
                None,
                None,
                None,
                None,
            )
            .unwrap();

        let task_id = db
            .create_task(
                Some(project_id),
                Some(goal_id),
                "Task",
                None,
                None,
                None,
            )
            .unwrap();

        db.delete_goal(goal_id).unwrap();

        // Task should still exist
        let task = db.get_task_by_id(task_id).unwrap().unwrap();
        assert_eq!(task.title, "Task");
        // goal_id should be cleared (orphan task)
        assert_eq!(task.goal_id, None);
    }

    // ═══════════════════════════════════════
    // Project Isolation Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_task_project_isolation() {
        let (db, project1) = setup_test_db();
        let (project2, _) = db.get_or_create_project("/other/path", Some("other")).unwrap();

        db.create_task(
            Some(project1),
            None,
            "Project 1 task",
            None,
            None,
            None,
        )
        .unwrap();
        db.create_task(
            Some(project2),
            None,
            "Project 2 task",
            None,
            None,
            None,
        )
        .unwrap();

        let tasks1 = db.get_tasks(Some(project1), None).unwrap();
        let tasks2 = db.get_tasks(Some(project2), None).unwrap();

        assert_eq!(tasks1.len(), 1);
        assert_eq!(tasks2.len(), 1);
        assert_eq!(tasks1[0].title, "Project 1 task");
        assert_eq!(tasks2[0].title, "Project 2 task");
    }

    #[test]
    fn test_goal_project_isolation() {
        let (db, project1) = setup_test_db();
        let (project2, _) = db.get_or_create_project("/other/path", Some("other")).unwrap();

        db.create_goal(
            Some(project1),
            "Project 1 goal",
            None,
            None,
            None,
            None,
        )
        .unwrap();
        db.create_goal(
            Some(project2),
            "Project 2 goal",
            None,
            None,
            None,
            None,
        )
        .unwrap();

        let goals1 = db.get_goals(Some(project1), None).unwrap();
        let goals2 = db.get_goals(Some(project2), None).unwrap();

        assert_eq!(goals1.len(), 1);
        assert_eq!(goals2.len(), 1);
        assert_eq!(goals1[0].title, "Project 1 goal");
        assert_eq!(goals2[0].title, "Project 2 goal");
    }

    // ═══════════════════════════════════════
    // Edge Cases
    // ═══════════════════════════════════════

    #[test]
    fn test_empty_title_task() {
        let (db, project_id) = setup_test_db();

        // Empty title should still work
        let id = db
            .create_task(Some(project_id), None, "", None, None, None)
            .unwrap();

        assert!(id > 0);
    }

    #[test]
    fn test_empty_title_goal() {
        let (db, project_id) = setup_test_db();

        // Empty title should still work
        let id = db
            .create_goal(Some(project_id), "", None, None, None, None)
            .unwrap();

        assert!(id > 0);
    }

    #[test]
    fn test_invalid_progress_percent() {
        let (db, project_id) = setup_test_db();

        // Should handle values outside 0-100 range
        let id = db
            .create_goal(
                Some(project_id),
                "Goal",
                None,
                None,
                None,
                Some(150),
            )
            .unwrap();

        let goal = db.get_goal_by_id(id).unwrap().unwrap();
        assert_eq!(goal.progress_percent, 150);
    }

    #[test]
    fn test_negative_progress_percent() {
        let (db, project_id) = setup_test_db();

        let id = db
            .create_goal(
                Some(project_id),
                "Goal",
                None,
                None,
                None,
                Some(-10),
            )
            .unwrap();

        let goal = db.get_goal_by_id(id).unwrap().unwrap();
        assert_eq!(goal.progress_percent, -10);
    }
}
