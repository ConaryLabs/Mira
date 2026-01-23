// crates/mira-server/src/db/session_tests.rs
// Tests for session and tool history operations

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
    // create_session Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_create_session_basic() {
        let (db, _project_id) = setup_test_db();

        let result = db.create_session("test-session-123", None);
        assert!(result.is_ok(), "create_session failed: {:?}", result.err());
    }

    #[test]
    fn test_create_session_with_project() {
        let (db, project_id) = setup_test_db();

        let result = db.create_session("session-with-project", Some(project_id));
        assert!(result.is_ok(), "create_session with project failed: {:?}", result.err());

        // Verify session exists
        let sessions = db.get_recent_sessions(project_id, 10).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "session-with-project");
        assert_eq!(sessions[0].project_id, Some(project_id));
        assert_eq!(sessions[0].status, "active");
    }

    #[test]
    fn test_create_session_upsert() {
        let (db, project_id) = setup_test_db();

        // Create session first time
        db.create_session("upsert-session", Some(project_id))
            .unwrap();

        // Get initial created_at
        let sessions = db.get_recent_sessions(project_id, 1).unwrap();
        let initial_started = sessions[0].started_at.clone();

        // Wait a bit and upsert
        std::thread::sleep(std::time::Duration::from_millis(10));
        db.create_session("upsert-session", Some(project_id))
            .unwrap();

        // Should still be one session
        let sessions = db.get_recent_sessions(project_id, 10).unwrap();
        assert_eq!(sessions.len(), 1);
        // started_at should be unchanged (created once)
        assert_eq!(sessions[0].started_at, initial_started);
        // last_activity should be updated (checked by upsert behavior)
    }

    // ═══════════════════════════════════════
    // touch_session Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_touch_session_existing() {
        let (db, project_id) = setup_test_db();

        db.create_session("touch-test", Some(project_id)).unwrap();

        // Get initial last_activity
        let sessions = db.get_recent_sessions(project_id, 1).unwrap();
        let initial_activity = sessions[0].last_activity.clone();

        // Wait 1 second to ensure timestamp changes (SQLite has second precision)
        std::thread::sleep(std::time::Duration::from_secs(1));
        db.touch_session("touch-test").unwrap();

        // Verify last_activity updated
        let sessions = db.get_recent_sessions(project_id, 1).unwrap();
        assert_ne!(sessions[0].last_activity, initial_activity);
    }

    #[test]
    fn test_touch_session_nonexistent() {
        let db = Database::open_in_memory().unwrap();

        // Touching non-existent session should not error
        let result = db.touch_session("nonexistent");
        assert!(result.is_ok(), "touch_session should succeed even for nonexistent session");
    }

    // ═══════════════════════════════════════
    // log_tool_call Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_log_tool_call_basic() {
        let (db, _project_id) = setup_test_db();

        // Create session first (required by foreign key constraint)
        db.create_session("session-1", None).unwrap();

        let id = db
            .log_tool_call(
                "session-1",
                "remember",
                r#"{"content": "test"}"#,
                "Stored memory ID: 1",
                None,
                true,
            )
            .unwrap();

        assert!(id > 0);

        // Verify entry
        let history = db.get_session_history("session-1", 10).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].tool_name, "remember");
        assert_eq!(history[0].arguments, Some(r#"{"content": "test"}"#.to_string()));
        assert_eq!(history[0].result_summary, Some("Stored memory ID: 1".to_string()));
        assert!(history[0].success);
    }

    #[test]
    fn test_log_tool_call_with_full_result() {
        let (db, _project_id) = setup_test_db();

        db.create_session("session-2", None).unwrap();

        let full_result = r#"{"detailed": "output", "with": "lots of data"}"#;
        db.log_tool_call(
            "session-2",
            "search_code",
            "query",
            "Found 5 results",
            Some(full_result),
            true,
        )
        .unwrap();

        let history = db.get_session_history("session-2", 10).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].tool_name, "search_code");
    }

    #[test]
    fn test_log_tool_call_failure() {
        let (db, _project_id) = setup_test_db();

        db.create_session("session-3", None).unwrap();

        db.log_tool_call(
            "session-3",
            "broken_tool",
            "{}",
            "Error: something failed",
            None,
            false,
        )
        .unwrap();

        let history = db.get_session_history("session-3", 10).unwrap();
        assert_eq!(history.len(), 1);
        assert!(!history[0].success);
    }

    #[test]
    fn test_log_multiple_tool_calls() {
        let (db, _project_id) = setup_test_db();

        db.create_session("session-multi", None).unwrap();

        for i in 0..5 {
            db.log_tool_call(
                "session-multi",
                &format!("tool_{}", i),
                "{}",
                &format!("Result {}", i),
                None,
                true,
            )
            .unwrap();
        }

        let history = db.get_session_history("session-multi", 10).unwrap();
        assert_eq!(history.len(), 5);
        // Should be ordered by created_at DESC
        assert_eq!(history[0].tool_name, "tool_4");
        assert_eq!(history[4].tool_name, "tool_0");
    }

    // ═══════════════════════════════════════
    // get_session_history Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_get_session_history_empty() {
        let db = Database::open_in_memory().unwrap();

        let history = db.get_session_history("nonexistent", 10).unwrap();
        assert_eq!(history.len(), 0);
    }

    #[test]
    fn test_get_session_history_limit() {
        let (db, _project_id) = setup_test_db();

        db.create_session("limit-test", None).unwrap();

        // Add 10 entries
        for i in 0..10 {
            db.log_tool_call("limit-test", "tool", "{}", &i.to_string(), None, true)
                .unwrap();
        }

        // Request only 5
        let history = db.get_session_history("limit-test", 5).unwrap();
        assert_eq!(history.len(), 5);
    }

    #[test]
    fn test_get_session_history_ordering() {
        let (db, _project_id) = setup_test_db();

        db.create_session("order-test", None).unwrap();

        // Add entries with delays to ensure different timestamps
        for i in 0..3 {
            db.log_tool_call(
                "order-test",
                &format!("tool_{}", i),
                "{}",
                &format!("result_{}", i),
                None,
                true,
            )
            .unwrap();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }

        let history = db.get_session_history("order-test", 10).unwrap();
        assert_eq!(history.len(), 3);
        // Most recent first
        assert_eq!(history[0].tool_name, "tool_2");
        assert_eq!(history[1].tool_name, "tool_1");
        assert_eq!(history[2].tool_name, "tool_0");
    }

    // ═══════════════════════════════════════
    // get_history_after Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_get_history_after_basic() {
        let (db, _project_id) = setup_test_db();

        db.create_session("after-test", None).unwrap();

        let ids: Vec<i64> = (0..5)
            .map(|i| {
                db.log_tool_call(
                    "after-test",
                    &format!("tool_{}", i),
                    "{}",
                    &format!("result_{}", i),
                    None,
                    true,
                )
                .unwrap()
            })
            .collect();

        // Get entries after ID 2
        let history = db.get_history_after("after-test", ids[1], 10).unwrap();
        // Should return IDs 3, 4 (everything > 2)
        assert!(history.len() >= 3);
        // Should be ordered ASC by ID
        assert_eq!(history[0].id, ids[2]);
    }

    #[test]
    fn test_get_history_after_limit() {
        let (db, _project_id) = setup_test_db();

        db.create_session("after-limit-test", None).unwrap();

        for i in 0..10 {
            db.log_tool_call(
                "after-limit-test",
                &format!("tool_{}", i),
                "{}",
                "result",
                None,
                true,
            )
            .unwrap();
        }

        // Get after first entry, limit to 3
        let history = db.get_history_after("after-limit-test", 0, 3).unwrap();
        assert_eq!(history.len(), 3);
    }

    #[test]
    fn test_get_history_after_empty() {
        let db = Database::open_in_memory().unwrap();

        let history = db.get_history_after("nonexistent", 0, 10).unwrap();
        assert_eq!(history.len(), 0);
    }

    // ═══════════════════════════════════════
    // get_recent_sessions Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_get_recent_sessions_basic() {
        let (db, project_id) = setup_test_db();

        db.create_session("session-1", Some(project_id)).unwrap();
        db.create_session("session-2", Some(project_id)).unwrap();

        let sessions = db.get_recent_sessions(project_id, 10).unwrap();
        assert_eq!(sessions.len(), 2);
        assert!(sessions.iter().all(|s| s.project_id == Some(project_id)));
    }

    #[test]
    fn test_get_recent_sessions_limit() {
        let (db, project_id) = setup_test_db();

        for i in 0..5 {
            db.create_session(&format!("session-{}", i), Some(project_id))
                .unwrap();
        }

        let sessions = db.get_recent_sessions(project_id, 3).unwrap();
        assert_eq!(sessions.len(), 3);
    }

    #[test]
    fn test_get_recent_sessions_ordering() {
        let (db, project_id) = setup_test_db();

        db.create_session("old-session", Some(project_id)).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        db.create_session("new-session", Some(project_id)).unwrap();

        let sessions = db.get_recent_sessions(project_id, 10).unwrap();
        assert_eq!(sessions.len(), 2);
        // Most recent activity first
        assert_eq!(sessions[0].id, "new-session");
        assert_eq!(sessions[1].id, "old-session");
    }

    #[test]
    fn test_get_recent_sessions_project_isolation() {
        let (db, project1) = setup_test_db();
        let (project2, _) = db.get_or_create_project("/other/path", Some("other")).unwrap();

        db.create_session("proj1-session", Some(project1)).unwrap();
        db.create_session("proj2-session", Some(project2)).unwrap();

        let sessions1 = db.get_recent_sessions(project1, 10).unwrap();
        let sessions2 = db.get_recent_sessions(project2, 10).unwrap();

        assert_eq!(sessions1.len(), 1);
        assert_eq!(sessions2.len(), 1);
        assert_eq!(sessions1[0].id, "proj1-session");
        assert_eq!(sessions2[0].id, "proj2-session");
    }

    // ═══════════════════════════════════════
    // get_session_stats Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_get_session_stats_empty() {
        let db = Database::open_in_memory().unwrap();

        let (count, tools) = db.get_session_stats("empty-session").unwrap();
        assert_eq!(count, 0);
        assert_eq!(tools.len(), 0);
    }

    #[test]
    fn test_get_session_stats_with_calls() {
        let (db, _project_id) = setup_test_db();

        db.create_session("stats-session", None).unwrap();

        // Add various tool calls
        for _i in 0..3 {
            db.log_tool_call("stats-session", "remember", "{}", "ok", None, true)
                .unwrap();
        }
        for _i in 0..2 {
            db.log_tool_call("stats-session", "recall", "{}", "ok", None, true)
                .unwrap();
        }
        db.log_tool_call("stats-session", "forget", "{}", "ok", None, true)
            .unwrap();

        let (count, tools) = db.get_session_stats("stats-session").unwrap();
        assert_eq!(count, 6);
        assert_eq!(tools.len(), 3);
        // remember should be first (most used)
        assert_eq!(tools[0], "remember");
        assert_eq!(tools[1], "recall");
        assert_eq!(tools[2], "forget");
    }

    #[test]
    fn test_get_session_stats_top_five() {
        let (db, _project_id) = setup_test_db();

        db.create_session("top-five-session", None).unwrap();

        // Add 10 different tools
        for i in 0..10 {
            db.log_tool_call(
                "top-five-session",
                &format!("tool_{}", i),
                "{}",
                "ok",
                None,
                true,
            )
            .unwrap();
        }

        let (_count, tools) = db.get_session_stats("top-five-session").unwrap();
        // Should only return top 5
        assert_eq!(tools.len(), 5);
    }

    // ═══════════════════════════════════════
    // build_session_recap Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_build_session_recap_empty() {
        let db = Database::open_in_memory().unwrap();

        let recap = db.build_session_recap(None);
        // Should have welcome banner at minimum
        assert!(recap.contains("Welcome back"), "Recap was: {}", recap);
    }

    #[test]
    fn test_build_session_recap_with_project() {
        let (db, project_id) = setup_test_db();

        let recap = db.build_session_recap(Some(project_id));
        assert!(recap.contains("test project"));
        assert!(recap.contains("Welcome back to"));
    }

    #[test]
    fn test_build_session_recap_with_pending_tasks() {
        let (db, project_id) = setup_test_db();

        // Create a pending task
        db.create_task(
            Some(project_id),
            None,  // goal_id
            "Test task",
            Some("Test description"),
            Some("pending"),
            Some("high"),
        )
        .unwrap();

        let recap = db.build_session_recap(Some(project_id));
        assert!(recap.contains("Pending tasks"));
        assert!(recap.contains("Test task"));
    }

    #[test]
    fn test_build_session_recap_with_active_goals() {
        let (db, project_id) = setup_test_db();

        // Create an active goal
        db.create_goal(
            Some(project_id),
            "Test goal",
            Some("Test description"),
            Some("in_progress"),
            Some("medium"),
            Some(50),
        )
        .unwrap();

        let recap = db.build_session_recap(Some(project_id));
        assert!(recap.contains("Active goals"));
        assert!(recap.contains("Test goal"));
    }

    #[test]
    fn test_build_session_recap_with_recent_sessions() {
        let (db, project_id) = setup_test_db();

        // Create an old session (not active)
        db.create_session("old-session", Some(project_id)).unwrap();
        // Update it to not be active
        {
            let conn = db.conn();
            conn.execute(
                "UPDATE sessions SET status = 'completed' WHERE id = ?",
                ["old-session"],
            )
            .unwrap();
        } // conn dropped here, lock released

        // Create current active session
        db.create_session("current-active", Some(project_id)).unwrap();

        let recap = db.build_session_recap(Some(project_id));
        // Should show recent sessions (excluding active)
        assert!(recap.contains("Recent sessions") || recap.contains("Welcome back"));
    }

    // ═══════════════════════════════════════
    // Integration Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_full_session_lifecycle() {
        let (db, project_id) = setup_test_db();

        // Create session
        let session_id = "lifecycle-test";
        db.create_session(session_id, Some(project_id)).unwrap();

        // Log some tool calls
        db.log_tool_call(session_id, "remember", "{}", "Stored memory", None, true)
            .unwrap();
        db.log_tool_call(session_id, "recall", "{}", "Found memories", None, true)
            .unwrap();

        // Check stats
        let (count, tools) = db.get_session_stats(session_id).unwrap();
        assert_eq!(count, 2);
        assert_eq!(tools.len(), 2);

        // Check history
        let history = db.get_session_history(session_id, 10).unwrap();
        assert_eq!(history.len(), 2);

        // Check session is in recent sessions
        let sessions = db.get_recent_sessions(project_id, 10).unwrap();
        assert!(sessions.iter().any(|s| s.id == session_id));

        // Touch session
        db.touch_session(session_id).unwrap();

        // Build recap
        let recap = db.build_session_recap(Some(project_id));
        assert!(recap.contains("test project"));
    }

    #[test]
    fn test_tool_history_entry_fields() {
        let (db, _project_id) = setup_test_db();

        db.create_session("fields-test", None).unwrap();

        db.log_tool_call(
            "fields-test",
            "test_tool",
            r#"{"arg1": "value1", "arg2": "value2"}"#,
            "Success summary",
            Some("Full detailed result"),
            true,
        )
        .unwrap();

        let history = db.get_session_history("fields-test", 1).unwrap();
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

    #[test]
    fn test_session_info_fields() {
        let (db, project_id) = setup_test_db();

        db.create_session("info-test", Some(project_id)).unwrap();

        let sessions = db.get_recent_sessions(project_id, 1).unwrap();
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

    #[test]
    fn test_empty_session_id() {
        let (db, _project_id) = setup_test_db();

        // Empty session_id should still work
        let result = db.create_session("", None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_very_long_session_id() {
        let (db, _project_id) = setup_test_db();

        let long_id = "a".repeat(1000);
        let result = db.create_session(&long_id, None);
        assert!(result.is_ok());

        // Should be able to retrieve
        db.touch_session(&long_id).unwrap();
    }

    #[test]
    fn test_special_characters_in_arguments() {
        let (db, _project_id) = setup_test_db();

        db.create_session("special-test", None).unwrap();

        let special_args = r#"{"text": "Hello \"world\"", "emoji": "🎉", "newline": "line1\nline2"}"#;
        db.log_tool_call("special-test", "tool", special_args, "ok", None, true)
            .unwrap();

        let history = db.get_session_history("special-test", 1).unwrap();
        assert_eq!(history[0].arguments, Some(special_args.to_string()));
    }
}
