// db/session_goals_tests.rs
// Tests for session-goal linkage

use super::test_support::{seed_goal, seed_session, setup_test_pool_with_project};
use super::{
    count_sessions_for_goal_sync, delete_goal_sync, delete_session_goals_for_goal_sync,
    get_goals_for_session_sync, get_sessions_for_goal_sync, record_session_goal_sync,
};

#[cfg(test)]
mod tests {
    use super::*;

    // ═══════════════════════════════════════
    // record_session_goal_sync Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_record_session_goal_basic() {
        let (pool, project_id) = setup_test_pool_with_project().await;

        let created = db!(pool, |conn| {
            seed_session(conn, "sess-1", project_id, "active");
            let goal_id = seed_goal(conn, project_id, "Test Goal", "planning", 0);
            record_session_goal_sync(conn, "sess-1", goal_id, "created").map_err(Into::into)
        });

        assert!(created, "First insert should return true");
    }

    #[tokio::test]
    async fn test_record_session_goal_idempotent() {
        let (pool, project_id) = setup_test_pool_with_project().await;

        let (first, second) = db!(pool, |conn| {
            seed_session(conn, "sess-1", project_id, "active");
            let goal_id = seed_goal(conn, project_id, "Test Goal", "planning", 0);
            let first = record_session_goal_sync(conn, "sess-1", goal_id, "created").unwrap();
            let second = record_session_goal_sync(conn, "sess-1", goal_id, "created").unwrap();
            Ok::<_, anyhow::Error>((first, second))
        });

        assert!(first, "First insert should return true");
        assert!(!second, "Duplicate insert should return false");
    }

    #[tokio::test]
    async fn test_record_session_goal_multiple_interaction_types() {
        let (pool, project_id) = setup_test_pool_with_project().await;

        let (created, updated, milestone) = db!(pool, |conn| {
            seed_session(conn, "sess-1", project_id, "active");
            let goal_id = seed_goal(conn, project_id, "Test Goal", "planning", 0);
            let created = record_session_goal_sync(conn, "sess-1", goal_id, "created").unwrap();
            let updated = record_session_goal_sync(conn, "sess-1", goal_id, "updated").unwrap();
            let milestone =
                record_session_goal_sync(conn, "sess-1", goal_id, "milestone_completed").unwrap();
            Ok::<_, anyhow::Error>((created, updated, milestone))
        });

        assert!(created, "created type should insert");
        assert!(
            updated,
            "updated type should insert (different interaction_type)"
        );
        assert!(milestone, "milestone_completed type should insert");
    }

    // ═══════════════════════════════════════
    // get_sessions_for_goal_sync Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_get_sessions_for_goal() {
        let (pool, project_id) = setup_test_pool_with_project().await;

        let links = db!(pool, |conn| {
            seed_session(conn, "sess-1", project_id, "active");
            seed_session(conn, "sess-2", project_id, "active");
            let goal_id = seed_goal(conn, project_id, "Shared Goal", "in_progress", 25);

            record_session_goal_sync(conn, "sess-1", goal_id, "created").unwrap();
            record_session_goal_sync(conn, "sess-2", goal_id, "updated").unwrap();

            get_sessions_for_goal_sync(conn, goal_id, 10).map_err(Into::into)
        });

        assert_eq!(links.len(), 2);
        // Both sessions should be present (order may vary due to same-second timestamps)
        let session_ids: Vec<&str> = links.iter().map(|l| l.session_id.as_str()).collect();
        assert!(session_ids.contains(&"sess-1"));
        assert!(session_ids.contains(&"sess-2"));
    }

    #[tokio::test]
    async fn test_get_sessions_for_goal_respects_limit() {
        let (pool, project_id) = setup_test_pool_with_project().await;

        let links = db!(pool, |conn| {
            let goal_id = seed_goal(conn, project_id, "Goal", "in_progress", 0);
            for i in 0..5 {
                let sid = format!("sess-{}", i);
                seed_session(conn, &sid, project_id, "active");
                record_session_goal_sync(conn, &sid, goal_id, "updated").unwrap();
            }
            get_sessions_for_goal_sync(conn, goal_id, 3).map_err(Into::into)
        });

        assert_eq!(links.len(), 3);
    }

    #[tokio::test]
    async fn test_get_sessions_for_goal_empty() {
        let (pool, project_id) = setup_test_pool_with_project().await;

        let links = db!(pool, |conn| {
            let goal_id = seed_goal(conn, project_id, "Lonely Goal", "planning", 0);
            get_sessions_for_goal_sync(conn, goal_id, 10).map_err(Into::into)
        });

        assert!(links.is_empty());
    }

    // ═══════════════════════════════════════
    // get_goals_for_session_sync Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_get_goals_for_session() {
        let (pool, project_id) = setup_test_pool_with_project().await;

        let links = db!(pool, |conn| {
            seed_session(conn, "sess-1", project_id, "active");
            let goal_a = seed_goal(conn, project_id, "Goal A", "in_progress", 10);
            let goal_b = seed_goal(conn, project_id, "Goal B", "planning", 0);

            record_session_goal_sync(conn, "sess-1", goal_a, "updated").unwrap();
            record_session_goal_sync(conn, "sess-1", goal_b, "created").unwrap();

            get_goals_for_session_sync(conn, "sess-1").map_err(Into::into)
        });

        assert_eq!(links.len(), 2);
        let goal_ids: Vec<i64> = links.iter().map(|l| l.goal_id).collect();
        // Both goals should be present (order is by created_at DESC)
        assert_eq!(goal_ids.len(), 2);
    }

    #[tokio::test]
    async fn test_get_goals_for_session_empty() {
        let (pool, project_id) = setup_test_pool_with_project().await;

        let links = db!(pool, |conn| {
            seed_session(conn, "sess-1", project_id, "active");
            get_goals_for_session_sync(conn, "sess-1").map_err(Into::into)
        });

        assert!(links.is_empty());
    }

    // ═══════════════════════════════════════
    // count_sessions_for_goal_sync Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_count_sessions_for_goal() {
        let (pool, project_id) = setup_test_pool_with_project().await;

        let count = db!(pool, |conn| {
            let goal_id = seed_goal(conn, project_id, "Popular Goal", "in_progress", 50);
            seed_session(conn, "sess-1", project_id, "active");
            seed_session(conn, "sess-2", project_id, "active");
            seed_session(conn, "sess-3", project_id, "active");

            record_session_goal_sync(conn, "sess-1", goal_id, "created").unwrap();
            record_session_goal_sync(conn, "sess-2", goal_id, "updated").unwrap();
            record_session_goal_sync(conn, "sess-3", goal_id, "milestone_completed").unwrap();
            // Duplicate from sess-1 with different type — should not double-count session
            record_session_goal_sync(conn, "sess-1", goal_id, "updated").unwrap();

            count_sessions_for_goal_sync(conn, goal_id).map_err(Into::into)
        });

        assert_eq!(count, 3, "Should count distinct sessions, not total rows");
    }

    #[tokio::test]
    async fn test_count_sessions_for_goal_zero() {
        let (pool, project_id) = setup_test_pool_with_project().await;

        let count = db!(pool, |conn| {
            let goal_id = seed_goal(conn, project_id, "New Goal", "planning", 0);
            count_sessions_for_goal_sync(conn, goal_id).map_err(Into::into)
        });

        assert_eq!(count, 0);
    }

    // ═══════════════════════════════════════
    // delete_session_goals_for_goal_sync Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_delete_session_goals_for_goal() {
        let (pool, project_id) = setup_test_pool_with_project().await;

        let (deleted, remaining) = db!(pool, |conn| {
            let goal_id = seed_goal(conn, project_id, "Goal to clean", "in_progress", 30);
            seed_session(conn, "sess-1", project_id, "active");
            seed_session(conn, "sess-2", project_id, "active");

            record_session_goal_sync(conn, "sess-1", goal_id, "created").unwrap();
            record_session_goal_sync(conn, "sess-2", goal_id, "updated").unwrap();

            let deleted = delete_session_goals_for_goal_sync(conn, goal_id).unwrap();
            let remaining = get_sessions_for_goal_sync(conn, goal_id, 10).unwrap();
            Ok::<_, anyhow::Error>((deleted, remaining))
        });

        assert_eq!(deleted, 2, "Should delete both links");
        assert!(remaining.is_empty(), "No links should remain");
    }

    #[tokio::test]
    async fn test_delete_session_goals_for_nonexistent_goal() {
        let (pool, _project_id) = setup_test_pool_with_project().await;

        let deleted = db!(pool, |conn| {
            delete_session_goals_for_goal_sync(conn, 99999).map_err(Into::into)
        });

        assert_eq!(deleted, 0, "Should delete nothing for nonexistent goal");
    }

    // ═══════════════════════════════════════
    // delete_goal_sync Cascade Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_delete_goal_cascades_session_goals() {
        let (pool, project_id) = setup_test_pool_with_project().await;

        let remaining = db!(pool, |conn| {
            let goal_id = seed_goal(conn, project_id, "Doomed Goal", "in_progress", 50);
            seed_session(conn, "sess-1", project_id, "active");
            seed_session(conn, "sess-2", project_id, "active");

            record_session_goal_sync(conn, "sess-1", goal_id, "created").unwrap();
            record_session_goal_sync(conn, "sess-2", goal_id, "updated").unwrap();

            // Delete the goal — should cascade to session_goals
            delete_goal_sync(conn, goal_id).unwrap();

            get_sessions_for_goal_sync(conn, goal_id, 10).map_err(Into::into)
        });

        assert!(
            remaining.is_empty(),
            "session_goals should be cleaned up when goal is deleted"
        );
    }

    // ═══════════════════════════════════════
    // Retention Orphan Cleanup Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_retention_orphan_cleanup_session_goals() {
        let (pool, project_id) = setup_test_pool_with_project().await;

        let (count_before, cleaned, count_after) = db!(pool, |conn| {
            // Create a valid session + goal + link
            seed_session(conn, "valid-sess", project_id, "active");
            let valid_goal = seed_goal(conn, project_id, "Valid Goal", "planning", 0);
            record_session_goal_sync(conn, "valid-sess", valid_goal, "created").unwrap();

            // Create orphan scenario 1: session that will be deleted
            seed_session(conn, "ghost-sess", project_id, "active");
            record_session_goal_sync(conn, "ghost-sess", valid_goal, "updated").unwrap();

            // Create orphan scenario 2: goal that will be deleted
            let doomed_goal = seed_goal(conn, project_id, "Doomed Goal", "planning", 0);
            record_session_goal_sync(conn, "valid-sess", doomed_goal, "created").unwrap();

            // Now create orphans by deleting parents directly (disable FK to bypass cascade)
            conn.execute_batch("PRAGMA foreign_keys = OFF;").unwrap();
            conn.execute("DELETE FROM sessions WHERE id = 'ghost-sess'", [])
                .unwrap();
            conn.execute(
                "DELETE FROM goals WHERE id = ?",
                rusqlite::params![doomed_goal],
            )
            .unwrap();
            conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();

            let count_before: i64 = conn
                .query_row("SELECT COUNT(*) FROM session_goals", [], |r| r.get(0))
                .unwrap();

            let cleaned = super::super::cleanup_orphans(conn).unwrap();

            let count_after: i64 = conn
                .query_row("SELECT COUNT(*) FROM session_goals", [], |r| r.get(0))
                .unwrap();

            Ok::<_, anyhow::Error>((count_before, cleaned, count_after))
        });

        assert_eq!(
            count_before, 3,
            "Should have 3 rows before cleanup (1 valid + 2 orphans)"
        );
        assert!(
            cleaned >= 2,
            "Should clean at least the 2 orphan session_goals rows"
        );
        assert_eq!(count_after, 1, "Only the valid link should remain");
    }
}
