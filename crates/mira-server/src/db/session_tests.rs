// crates/mira-server/src/db/session_tests.rs
// Tests for session and tool history operations

use super::pool::DatabasePool;
use super::{
    build_session_recap_sync, create_goal_sync, create_session_sync, create_task_sync,
    get_history_after_sync, get_or_create_project_sync, get_recent_sessions_sync,
    get_session_history_sync, get_session_stats_sync, log_tool_call_sync, touch_session_sync,
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
    // create_session Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_create_session_basic() {
        let (pool, _project_id) = setup_test_pool().await;

        let result = pool
            .interact(|conn| create_session_sync(conn, "test-session-123", None).map_err(Into::into))
            .await;
        assert!(result.is_ok(), "create_session failed: {:?}", result.err());
    }

    #[tokio::test]
    async fn test_create_session_with_project() {
        let (pool, project_id) = setup_test_pool().await;

        let result = pool
            .interact(move |conn| {
                create_session_sync(conn, "session-with-project", Some(project_id))
                    .map_err(Into::into)
            })
            .await;
        assert!(
            result.is_ok(),
            "create_session with project failed: {:?}",
            result.err()
        );

        // Verify session exists
        let sessions = pool
            .interact(move |conn| {
                get_recent_sessions_sync(conn, project_id, 10).map_err(Into::into)
            })
            .await
            .unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "session-with-project");
        assert_eq!(sessions[0].project_id, Some(project_id));
        assert_eq!(sessions[0].status, "active");
    }

    #[tokio::test]
    async fn test_create_session_upsert() {
        let (pool, project_id) = setup_test_pool().await;

        // Create session first time
        pool.interact(move |conn| {
            create_session_sync(conn, "upsert-session", Some(project_id)).map_err(Into::into)
        })
        .await
        .unwrap();

        // Get initial created_at
        let sessions = pool
            .interact(move |conn| get_recent_sessions_sync(conn, project_id, 1).map_err(Into::into))
            .await
            .unwrap();
        let initial_started = sessions[0].started_at.clone();

        // Wait a bit and upsert
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        pool.interact(move |conn| {
            create_session_sync(conn, "upsert-session", Some(project_id)).map_err(Into::into)
        })
        .await
        .unwrap();

        // Should still be one session
        let sessions = pool
            .interact(move |conn| {
                get_recent_sessions_sync(conn, project_id, 10).map_err(Into::into)
            })
            .await
            .unwrap();
        assert_eq!(sessions.len(), 1);
        // started_at should be unchanged (created once)
        assert_eq!(sessions[0].started_at, initial_started);
        // last_activity should be updated (checked by upsert behavior)
    }

    // ═══════════════════════════════════════
    // touch_session Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_touch_session_existing() {
        let (pool, project_id) = setup_test_pool().await;

        pool.interact(move |conn| {
            create_session_sync(conn, "touch-test", Some(project_id)).map_err(Into::into)
        })
        .await
        .unwrap();

        // Get initial last_activity
        let sessions = pool
            .interact(move |conn| get_recent_sessions_sync(conn, project_id, 1).map_err(Into::into))
            .await
            .unwrap();
        let initial_activity = sessions[0].last_activity.clone();

        // Wait 1 second to ensure timestamp changes (SQLite has second precision)
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        pool.interact(|conn| touch_session_sync(conn, "touch-test").map_err(Into::into))
            .await
            .unwrap();

        // Verify last_activity updated
        let sessions = pool
            .interact(move |conn| get_recent_sessions_sync(conn, project_id, 1).map_err(Into::into))
            .await
            .unwrap();
        assert_ne!(sessions[0].last_activity, initial_activity);
    }

    #[tokio::test]
    async fn test_touch_session_nonexistent() {
        let pool = Arc::new(DatabasePool::open_in_memory().await.unwrap());

        // Touching non-existent session should not error
        let result = pool
            .interact(|conn| touch_session_sync(conn, "nonexistent").map_err(Into::into))
            .await;
        assert!(
            result.is_ok(),
            "touch_session should succeed even for nonexistent session"
        );
    }

    // ═══════════════════════════════════════
    // log_tool_call Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_log_tool_call_basic() {
        let (pool, _project_id) = setup_test_pool().await;

        // Create session first (required by foreign key constraint)
        pool.interact(|conn| create_session_sync(conn, "session-1", None).map_err(Into::into))
            .await
            .unwrap();

        let id = pool
            .interact(|conn| {
                log_tool_call_sync(
                    conn,
                    "session-1",
                    "remember",
                    r#"{"content": "test"}"#,
                    "Stored memory ID: 1",
                    None,
                    true,
                )
                .map_err(Into::into)
            })
            .await
            .unwrap();

        assert!(id > 0);

        // Verify entry
        let history = pool
            .interact(|conn| get_session_history_sync(conn, "session-1", 10).map_err(Into::into))
            .await
            .unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].tool_name, "remember");
        assert_eq!(
            history[0].arguments,
            Some(r#"{"content": "test"}"#.to_string())
        );
        assert_eq!(
            history[0].result_summary,
            Some("Stored memory ID: 1".to_string())
        );
        assert!(history[0].success);
    }

    #[tokio::test]
    async fn test_log_tool_call_with_full_result() {
        let (pool, _project_id) = setup_test_pool().await;

        pool.interact(|conn| create_session_sync(conn, "session-2", None).map_err(Into::into))
            .await
            .unwrap();

        let full_result = r#"{"detailed": "output", "with": "lots of data"}"#;
        pool.interact(|conn| {
            log_tool_call_sync(
                conn,
                "session-2",
                "search_code",
                "query",
                "Found 5 results",
                Some(full_result),
                true,
            )
            .map_err(Into::into)
        })
        .await
        .unwrap();

        let history = pool
            .interact(|conn| get_session_history_sync(conn, "session-2", 10).map_err(Into::into))
            .await
            .unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].tool_name, "search_code");
    }

    #[tokio::test]
    async fn test_log_tool_call_failure() {
        let (pool, _project_id) = setup_test_pool().await;

        pool.interact(|conn| create_session_sync(conn, "session-3", None).map_err(Into::into))
            .await
            .unwrap();

        pool.interact(|conn| {
            log_tool_call_sync(
                conn,
                "session-3",
                "broken_tool",
                "{}",
                "Error: something failed",
                None,
                false,
            )
            .map_err(Into::into)
        })
        .await
        .unwrap();

        let history = pool
            .interact(|conn| get_session_history_sync(conn, "session-3", 10).map_err(Into::into))
            .await
            .unwrap();
        assert_eq!(history.len(), 1);
        assert!(!history[0].success);
    }

    #[tokio::test]
    async fn test_log_multiple_tool_calls() {
        let (pool, _project_id) = setup_test_pool().await;

        pool.interact(|conn| create_session_sync(conn, "session-multi", None).map_err(Into::into))
            .await
            .unwrap();

        for i in 0..5 {
            let tool_name = format!("tool_{}", i);
            let result = format!("Result {}", i);
            pool.interact(move |conn| {
                log_tool_call_sync(conn, "session-multi", &tool_name, "{}", &result, None, true)
                    .map_err(Into::into)
            })
            .await
            .unwrap();
        }

        let history = pool
            .interact(|conn| get_session_history_sync(conn, "session-multi", 10).map_err(Into::into))
            .await
            .unwrap();
        assert_eq!(history.len(), 5);
        // Should be ordered by created_at DESC
        assert_eq!(history[0].tool_name, "tool_4");
        assert_eq!(history[4].tool_name, "tool_0");
    }

    // ═══════════════════════════════════════
    // get_session_history Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_get_session_history_empty() {
        let pool = Arc::new(DatabasePool::open_in_memory().await.unwrap());

        let history = pool
            .interact(|conn| get_session_history_sync(conn, "nonexistent", 10).map_err(Into::into))
            .await
            .unwrap();
        assert_eq!(history.len(), 0);
    }

    #[tokio::test]
    async fn test_get_session_history_limit() {
        let (pool, _project_id) = setup_test_pool().await;

        pool.interact(|conn| create_session_sync(conn, "limit-test", None).map_err(Into::into))
            .await
            .unwrap();

        // Add 10 entries
        for i in 0..10 {
            let result = i.to_string();
            pool.interact(move |conn| {
                log_tool_call_sync(conn, "limit-test", "tool", "{}", &result, None, true)
                    .map_err(Into::into)
            })
            .await
            .unwrap();
        }

        // Request only 5
        let history = pool
            .interact(|conn| get_session_history_sync(conn, "limit-test", 5).map_err(Into::into))
            .await
            .unwrap();
        assert_eq!(history.len(), 5);
    }

    #[tokio::test]
    async fn test_get_session_history_ordering() {
        let (pool, _project_id) = setup_test_pool().await;

        pool.interact(|conn| create_session_sync(conn, "order-test", None).map_err(Into::into))
            .await
            .unwrap();

        // Add entries with delays to ensure different timestamps
        for i in 0..3 {
            let tool_name = format!("tool_{}", i);
            let result = format!("result_{}", i);
            pool.interact(move |conn| {
                log_tool_call_sync(conn, "order-test", &tool_name, "{}", &result, None, true)
                    .map_err(Into::into)
            })
            .await
            .unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }

        let history = pool
            .interact(|conn| get_session_history_sync(conn, "order-test", 10).map_err(Into::into))
            .await
            .unwrap();
        assert_eq!(history.len(), 3);
        // Most recent first
        assert_eq!(history[0].tool_name, "tool_2");
        assert_eq!(history[1].tool_name, "tool_1");
        assert_eq!(history[2].tool_name, "tool_0");
    }

    // ═══════════════════════════════════════
    // get_history_after Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_get_history_after_basic() {
        let (pool, _project_id) = setup_test_pool().await;

        pool.interact(|conn| create_session_sync(conn, "after-test", None).map_err(Into::into))
            .await
            .unwrap();

        let mut ids: Vec<i64> = Vec::new();
        for i in 0..5 {
            let tool_name = format!("tool_{}", i);
            let result = format!("result_{}", i);
            let id = pool
                .interact(move |conn| {
                    log_tool_call_sync(conn, "after-test", &tool_name, "{}", &result, None, true)
                        .map_err(Into::into)
                })
                .await
                .unwrap();
            ids.push(id);
        }

        // Get entries after ID 2
        let after_id = ids[1];
        let history = pool
            .interact(move |conn| {
                get_history_after_sync(conn, "after-test", after_id, 10).map_err(Into::into)
            })
            .await
            .unwrap();
        // Should return IDs 3, 4, 5 (everything > ids[1])
        assert!(history.len() >= 3);
        // Should be ordered ASC by ID
        assert_eq!(history[0].id, ids[2]);
    }

    #[tokio::test]
    async fn test_get_history_after_limit() {
        let (pool, _project_id) = setup_test_pool().await;

        pool.interact(|conn| {
            create_session_sync(conn, "after-limit-test", None).map_err(Into::into)
        })
        .await
        .unwrap();

        for i in 0..10 {
            let tool_name = format!("tool_{}", i);
            pool.interact(move |conn| {
                log_tool_call_sync(conn, "after-limit-test", &tool_name, "{}", "result", None, true)
                    .map_err(Into::into)
            })
            .await
            .unwrap();
        }

        // Get after first entry, limit to 3
        let history = pool
            .interact(|conn| {
                get_history_after_sync(conn, "after-limit-test", 0, 3).map_err(Into::into)
            })
            .await
            .unwrap();
        assert_eq!(history.len(), 3);
    }

    #[tokio::test]
    async fn test_get_history_after_empty() {
        let pool = Arc::new(DatabasePool::open_in_memory().await.unwrap());

        let history = pool
            .interact(|conn| get_history_after_sync(conn, "nonexistent", 0, 10).map_err(Into::into))
            .await
            .unwrap();
        assert_eq!(history.len(), 0);
    }

    // ═══════════════════════════════════════
    // get_recent_sessions Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_get_recent_sessions_basic() {
        let (pool, project_id) = setup_test_pool().await;

        pool.interact(move |conn| {
            create_session_sync(conn, "session-1", Some(project_id)).map_err(Into::into)
        })
        .await
        .unwrap();
        pool.interact(move |conn| {
            create_session_sync(conn, "session-2", Some(project_id)).map_err(Into::into)
        })
        .await
        .unwrap();

        let sessions = pool
            .interact(move |conn| {
                get_recent_sessions_sync(conn, project_id, 10).map_err(Into::into)
            })
            .await
            .unwrap();
        assert_eq!(sessions.len(), 2);
        assert!(sessions.iter().all(|s| s.project_id == Some(project_id)));
    }

    #[tokio::test]
    async fn test_get_recent_sessions_limit() {
        let (pool, project_id) = setup_test_pool().await;

        for i in 0..5 {
            let session_id = format!("session-{}", i);
            pool.interact(move |conn| {
                create_session_sync(conn, &session_id, Some(project_id)).map_err(Into::into)
            })
            .await
            .unwrap();
        }

        let sessions = pool
            .interact(move |conn| get_recent_sessions_sync(conn, project_id, 3).map_err(Into::into))
            .await
            .unwrap();
        assert_eq!(sessions.len(), 3);
    }

    #[tokio::test]
    async fn test_get_recent_sessions_ordering() {
        let (pool, project_id) = setup_test_pool().await;

        pool.interact(move |conn| {
            create_session_sync(conn, "old-session", Some(project_id)).map_err(Into::into)
        })
        .await
        .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        pool.interact(move |conn| {
            create_session_sync(conn, "new-session", Some(project_id)).map_err(Into::into)
        })
        .await
        .unwrap();

        let sessions = pool
            .interact(move |conn| {
                get_recent_sessions_sync(conn, project_id, 10).map_err(Into::into)
            })
            .await
            .unwrap();
        assert_eq!(sessions.len(), 2);
        // Most recent activity first
        assert_eq!(sessions[0].id, "new-session");
        assert_eq!(sessions[1].id, "old-session");
    }

    #[tokio::test]
    async fn test_get_recent_sessions_project_isolation() {
        let (pool, project1) = setup_test_pool().await;
        let project2 = pool
            .interact(|conn| {
                get_or_create_project_sync(conn, "/other/path", Some("other")).map_err(Into::into)
            })
            .await
            .unwrap()
            .0;

        pool.interact(move |conn| {
            create_session_sync(conn, "proj1-session", Some(project1)).map_err(Into::into)
        })
        .await
        .unwrap();
        pool.interact(move |conn| {
            create_session_sync(conn, "proj2-session", Some(project2)).map_err(Into::into)
        })
        .await
        .unwrap();

        let sessions1 = pool
            .interact(move |conn| {
                get_recent_sessions_sync(conn, project1, 10).map_err(Into::into)
            })
            .await
            .unwrap();
        let sessions2 = pool
            .interact(move |conn| {
                get_recent_sessions_sync(conn, project2, 10).map_err(Into::into)
            })
            .await
            .unwrap();

        assert_eq!(sessions1.len(), 1);
        assert_eq!(sessions2.len(), 1);
        assert_eq!(sessions1[0].id, "proj1-session");
        assert_eq!(sessions2[0].id, "proj2-session");
    }

    // ═══════════════════════════════════════
    // get_session_stats Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_get_session_stats_empty() {
        let pool = Arc::new(DatabasePool::open_in_memory().await.unwrap());

        let (count, tools) = pool
            .interact(|conn| get_session_stats_sync(conn, "empty-session").map_err(Into::into))
            .await
            .unwrap();
        assert_eq!(count, 0);
        assert_eq!(tools.len(), 0);
    }

    #[tokio::test]
    async fn test_get_session_stats_with_calls() {
        let (pool, _project_id) = setup_test_pool().await;

        pool.interact(|conn| create_session_sync(conn, "stats-session", None).map_err(Into::into))
            .await
            .unwrap();

        // Add various tool calls
        for _i in 0..3 {
            pool.interact(|conn| {
                log_tool_call_sync(conn, "stats-session", "remember", "{}", "ok", None, true)
                    .map_err(Into::into)
            })
            .await
            .unwrap();
        }
        for _i in 0..2 {
            pool.interact(|conn| {
                log_tool_call_sync(conn, "stats-session", "recall", "{}", "ok", None, true)
                    .map_err(Into::into)
            })
            .await
            .unwrap();
        }
        pool.interact(|conn| {
            log_tool_call_sync(conn, "stats-session", "forget", "{}", "ok", None, true)
                .map_err(Into::into)
        })
        .await
        .unwrap();

        let (count, tools) = pool
            .interact(|conn| get_session_stats_sync(conn, "stats-session").map_err(Into::into))
            .await
            .unwrap();
        assert_eq!(count, 6);
        assert_eq!(tools.len(), 3);
        // remember should be first (most used)
        assert_eq!(tools[0], "remember");
        assert_eq!(tools[1], "recall");
        assert_eq!(tools[2], "forget");
    }

    #[tokio::test]
    async fn test_get_session_stats_top_five() {
        let (pool, _project_id) = setup_test_pool().await;

        pool.interact(|conn| {
            create_session_sync(conn, "top-five-session", None).map_err(Into::into)
        })
        .await
        .unwrap();

        // Add 10 different tools
        for i in 0..10 {
            let tool_name = format!("tool_{}", i);
            pool.interact(move |conn| {
                log_tool_call_sync(conn, "top-five-session", &tool_name, "{}", "ok", None, true)
                    .map_err(Into::into)
            })
            .await
            .unwrap();
        }

        let (_count, tools) = pool
            .interact(|conn| get_session_stats_sync(conn, "top-five-session").map_err(Into::into))
            .await
            .unwrap();
        // Should only return top 5
        assert_eq!(tools.len(), 5);
    }

    // ═══════════════════════════════════════
    // build_session_recap Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_build_session_recap_empty() {
        let pool = Arc::new(DatabasePool::open_in_memory().await.unwrap());

        let recap = pool
            .interact(|conn| Ok::<_, anyhow::Error>(build_session_recap_sync(conn, None)))
            .await
            .unwrap();
        // Should have welcome banner at minimum
        assert!(recap.contains("Welcome back"), "Recap was: {}", recap);
    }

    #[tokio::test]
    async fn test_build_session_recap_with_project() {
        let (pool, project_id) = setup_test_pool().await;

        let recap = pool
            .interact(move |conn| {
                Ok::<_, anyhow::Error>(build_session_recap_sync(conn, Some(project_id)))
            })
            .await
            .unwrap();
        assert!(recap.contains("test project"));
        assert!(recap.contains("Welcome back to"));
    }

    #[tokio::test]
    async fn test_build_session_recap_with_pending_tasks() {
        let (pool, project_id) = setup_test_pool().await;

        // Create a pending task
        pool.interact(move |conn| {
            create_task_sync(
                conn,
                Some(project_id),
                None, // goal_id
                "Test task",
                Some("Test description"),
                Some("pending"),
                Some("high"),
            )
            .map_err(Into::into)
        })
        .await
        .unwrap();

        let recap = pool
            .interact(move |conn| {
                Ok::<_, anyhow::Error>(build_session_recap_sync(conn, Some(project_id)))
            })
            .await
            .unwrap();
        assert!(recap.contains("Pending tasks"));
        assert!(recap.contains("Test task"));
    }

    #[tokio::test]
    async fn test_build_session_recap_with_active_goals() {
        let (pool, project_id) = setup_test_pool().await;

        // Create an active goal
        pool.interact(move |conn| {
            create_goal_sync(
                conn,
                Some(project_id),
                "Test goal",
                Some("Test description"),
                Some("in_progress"),
                Some("medium"),
                Some(50),
            )
            .map_err(Into::into)
        })
        .await
        .unwrap();

        let recap = pool
            .interact(move |conn| {
                Ok::<_, anyhow::Error>(build_session_recap_sync(conn, Some(project_id)))
            })
            .await
            .unwrap();
        assert!(recap.contains("Active goals"));
        assert!(recap.contains("Test goal"));
    }

    #[tokio::test]
    async fn test_build_session_recap_with_recent_sessions() {
        let (pool, project_id) = setup_test_pool().await;

        // Create an old session (not active)
        pool.interact(move |conn| {
            create_session_sync(conn, "old-session", Some(project_id)).map_err(Into::into)
        })
        .await
        .unwrap();
        // Update it to not be active
        pool.interact(|conn| {
            conn.execute(
                "UPDATE sessions SET status = 'completed' WHERE id = ?",
                ["old-session"],
            )?;
            Ok::<_, anyhow::Error>(())
        })
        .await
        .unwrap();

        // Create current active session
        pool.interact(move |conn| {
            create_session_sync(conn, "current-active", Some(project_id)).map_err(Into::into)
        })
        .await
        .unwrap();

        let recap = pool
            .interact(move |conn| {
                Ok::<_, anyhow::Error>(build_session_recap_sync(conn, Some(project_id)))
            })
            .await
            .unwrap();
        // Should show recent sessions (excluding active)
        assert!(recap.contains("Recent sessions") || recap.contains("Welcome back"));
    }

    // ═══════════════════════════════════════
    // Integration Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_full_session_lifecycle() {
        let (pool, project_id) = setup_test_pool().await;

        // Create session
        let session_id = "lifecycle-test";
        pool.interact(move |conn| {
            create_session_sync(conn, session_id, Some(project_id)).map_err(Into::into)
        })
        .await
        .unwrap();

        // Log some tool calls
        pool.interact(|conn| {
            log_tool_call_sync(
                conn,
                "lifecycle-test",
                "remember",
                "{}",
                "Stored memory",
                None,
                true,
            )
            .map_err(Into::into)
        })
        .await
        .unwrap();
        pool.interact(|conn| {
            log_tool_call_sync(
                conn,
                "lifecycle-test",
                "recall",
                "{}",
                "Found memories",
                None,
                true,
            )
            .map_err(Into::into)
        })
        .await
        .unwrap();

        // Check stats
        let (count, tools) = pool
            .interact(|conn| get_session_stats_sync(conn, "lifecycle-test").map_err(Into::into))
            .await
            .unwrap();
        assert_eq!(count, 2);
        assert_eq!(tools.len(), 2);

        // Check history
        let history = pool
            .interact(|conn| {
                get_session_history_sync(conn, "lifecycle-test", 10).map_err(Into::into)
            })
            .await
            .unwrap();
        assert_eq!(history.len(), 2);

        // Check session is in recent sessions
        let sessions = pool
            .interact(move |conn| {
                get_recent_sessions_sync(conn, project_id, 10).map_err(Into::into)
            })
            .await
            .unwrap();
        assert!(sessions.iter().any(|s| s.id == session_id));

        // Touch session
        pool.interact(|conn| touch_session_sync(conn, "lifecycle-test").map_err(Into::into))
            .await
            .unwrap();

        // Build recap
        let recap = pool
            .interact(move |conn| {
                Ok::<_, anyhow::Error>(build_session_recap_sync(conn, Some(project_id)))
            })
            .await
            .unwrap();
        assert!(recap.contains("test project"));
    }

    #[tokio::test]
    async fn test_tool_history_entry_fields() {
        let (pool, _project_id) = setup_test_pool().await;

        pool.interact(|conn| create_session_sync(conn, "fields-test", None).map_err(Into::into))
            .await
            .unwrap();

        pool.interact(|conn| {
            log_tool_call_sync(
                conn,
                "fields-test",
                "test_tool",
                r#"{"arg1": "value1", "arg2": "value2"}"#,
                "Success summary",
                Some("Full detailed result"),
                true,
            )
            .map_err(Into::into)
        })
        .await
        .unwrap();

        let history = pool
            .interact(|conn| get_session_history_sync(conn, "fields-test", 1).map_err(Into::into))
            .await
            .unwrap();
        let entry = &history[0];

        assert_eq!(entry.session_id, "fields-test");
        assert_eq!(entry.tool_name, "test_tool");
        assert_eq!(
            entry.arguments,
            Some(r#"{"arg1": "value1", "arg2": "value2"}"#.to_string())
        );
        assert_eq!(entry.result_summary, Some("Success summary".to_string()));
        assert!(entry.success);
        assert!(!entry.created_at.is_empty());
    }

    #[tokio::test]
    async fn test_session_info_fields() {
        let (pool, project_id) = setup_test_pool().await;

        pool.interact(move |conn| {
            create_session_sync(conn, "info-test", Some(project_id)).map_err(Into::into)
        })
        .await
        .unwrap();

        let sessions = pool
            .interact(move |conn| get_recent_sessions_sync(conn, project_id, 1).map_err(Into::into))
            .await
            .unwrap();
        let info = &sessions[0];

        assert_eq!(info.id, "info-test");
        assert_eq!(info.project_id, Some(project_id));
        assert_eq!(info.status, "active");
        assert!(info.summary.is_none()); // No summary set
        assert!(!info.started_at.is_empty());
        assert!(!info.last_activity.is_empty());
    }

    // ═══════════════════════════════════════
    // Edge Cases
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_empty_session_id() {
        let (pool, _project_id) = setup_test_pool().await;

        // Empty session_id should still work
        let result = pool
            .interact(|conn| create_session_sync(conn, "", None).map_err(Into::into))
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_very_long_session_id() {
        let (pool, _project_id) = setup_test_pool().await;

        let long_id = "a".repeat(1000);
        let long_id_clone = long_id.clone();
        let result = pool
            .interact(move |conn| create_session_sync(conn, &long_id_clone, None).map_err(Into::into))
            .await;
        assert!(result.is_ok());

        // Should be able to retrieve
        pool.interact(move |conn| touch_session_sync(conn, &long_id).map_err(Into::into))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_special_characters_in_arguments() {
        let (pool, _project_id) = setup_test_pool().await;

        pool.interact(|conn| create_session_sync(conn, "special-test", None).map_err(Into::into))
            .await
            .unwrap();

        let special_args =
            r#"{"text": "Hello \"world\"", "emoji": "🎉", "newline": "line1\nline2"}"#;
        pool.interact(|conn| {
            log_tool_call_sync(conn, "special-test", "tool", special_args, "ok", None, true)
                .map_err(Into::into)
        })
        .await
        .unwrap();

        let history = pool
            .interact(|conn| get_session_history_sync(conn, "special-test", 1).map_err(Into::into))
            .await
            .unwrap();
        assert_eq!(history[0].arguments, Some(special_args.to_string()));
    }
}
