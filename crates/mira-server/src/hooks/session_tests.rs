// hooks/session_tests.rs
// Integration tests for build_resume_context building blocks

#[cfg(test)]
use crate::db::test_support::*;

/// Helper: run a sync closure on the pool, unwrapping the result.
#[cfg(test)]
async fn db<T: Send + 'static>(
    pool: &std::sync::Arc<crate::db::pool::DatabasePool>,
    f: impl FnOnce(&rusqlite::Connection) -> anyhow::Result<T> + Send + 'static,
) -> T {
    pool.interact(f).await.unwrap()
}

// =============================================================================
// Test 1: Project resolution uses cwd path
// =============================================================================

#[tokio::test]
async fn test_project_resolution_uses_cwd() {
    let (pool, project_a_id) = setup_test_pool_with_project().await;
    let project_b_id = setup_second_project(&pool).await;

    // Verify resolving /test/path returns project A, not B
    let resolved_a_id = db(&pool, |conn| {
        let (id, _name) = crate::db::get_or_create_project_sync(conn, "/test/path", None)?;
        Ok(id)
    })
    .await;
    assert_eq!(resolved_a_id, project_a_id);

    // Verify resolving /other/path returns project B, not A
    let resolved_b_id = db(&pool, |conn| {
        let (id, _name) = crate::db::get_or_create_project_sync(conn, "/other/path", None)?;
        Ok(id)
    })
    .await;
    assert_eq!(resolved_b_id, project_b_id);

    // Verify they are different
    assert_ne!(project_a_id, project_b_id);
}

// =============================================================================
// Test 2: Recent sessions returns completed only (filters out active)
// =============================================================================

#[tokio::test]
async fn test_recent_sessions_returns_completed_only() {
    let (pool, project_id) = setup_test_pool_with_project().await;

    db(&pool, move |conn| {
        seed_session(conn, "sess-active", project_id, "active");
        seed_session(conn, "sess-done", project_id, "completed");
        Ok(())
    })
    .await;

    let sessions = db(&pool, move |conn| {
        crate::db::get_recent_sessions_sync(conn, project_id, 10).map_err(Into::into)
    })
    .await;

    // get_recent_sessions_sync returns ALL sessions ordered by recency;
    // build_resume_context filters for status != "active" in-memory.
    // Verify at least the completed one is present.
    assert!(!sessions.is_empty());
    let completed: Vec<_> = sessions.iter().filter(|s| s.status != "active").collect();
    assert_eq!(completed.len(), 1);
    assert_eq!(completed[0].id, "sess-done");
}

// =============================================================================
// Test 3: No previous session returns empty
// =============================================================================

#[tokio::test]
async fn test_no_previous_session_returns_empty() {
    let (pool, project_id) = setup_test_pool_with_project().await;

    let sessions = db(&pool, move |conn| {
        crate::db::get_recent_sessions_sync(conn, project_id, 10).map_err(Into::into)
    })
    .await;

    assert!(sessions.is_empty(), "fresh project should have no sessions");
}

// =============================================================================
// Test 4: Tool history formatting
// =============================================================================

#[tokio::test]
async fn test_tool_history_formatting() {
    let (pool, project_id) = setup_test_pool_with_project().await;

    db(&pool, move |conn| {
        seed_session(conn, "sess-hist", project_id, "completed");
        seed_tool_history(conn, "sess-hist", "Read", r#"{"file_path":"/a.rs"}"#, "ok");
        seed_tool_history(conn, "sess-hist", "Edit", r#"{"file_path":"/b.rs"}"#, "ok");
        seed_tool_history(
            conn,
            "sess-hist",
            "Bash",
            r#"{"command":"cargo test"}"#,
            "ok",
        );
        seed_tool_history(conn, "sess-hist", "Grep", r#"{"pattern":"foo"}"#, "ok");
        seed_tool_history(conn, "sess-hist", "Write", r#"{"file_path":"/c.rs"}"#, "ok");
        Ok(())
    })
    .await;

    let history = db(&pool, |conn| {
        crate::db::get_session_history_sync(conn, "sess-hist", 10).map_err(Into::into)
    })
    .await;

    assert_eq!(history.len(), 5);
    // Verify tool names are present
    let tool_names: Vec<&str> = history.iter().map(|h| h.tool_name.as_str()).collect();
    assert!(tool_names.contains(&"Read"));
    assert!(tool_names.contains(&"Edit"));
    assert!(tool_names.contains(&"Bash"));
    assert!(tool_names.contains(&"Grep"));
    assert!(tool_names.contains(&"Write"));
    // All should be successful
    assert!(history.iter().all(|h| h.success));
}

// =============================================================================
// Test 5: Modified files extraction
// =============================================================================

#[tokio::test]
async fn test_modified_files_extraction() {
    let (pool, project_id) = setup_test_pool_with_project().await;

    db(&pool, move |conn| {
        seed_session(conn, "sess-files", project_id, "completed");
        // Write/Edit/NotebookEdit have file_path in arguments - should be extracted
        seed_tool_history(
            conn,
            "sess-files",
            "Write",
            r#"{"file_path":"/src/main.rs"}"#,
            "ok",
        );
        seed_tool_history(
            conn,
            "sess-files",
            "Edit",
            r#"{"file_path":"/src/lib.rs"}"#,
            "ok",
        );
        seed_tool_history(
            conn,
            "sess-files",
            "NotebookEdit",
            r#"{"file_path":"/notebooks/analysis.ipynb"}"#,
            "ok",
        );
        // Read and Bash should NOT appear in modified files
        seed_tool_history(
            conn,
            "sess-files",
            "Read",
            r#"{"file_path":"/src/other.rs"}"#,
            "ok",
        );
        seed_tool_history(conn, "sess-files", "Bash", r#"{"command":"ls"}"#, "ok");
        Ok(())
    })
    .await;

    let files = db(&pool, |conn| {
        Ok(crate::hooks::get_session_modified_files_sync(
            conn,
            "sess-files",
        ))
    })
    .await;

    assert_eq!(files.len(), 3);
    assert!(files.contains(&"/src/main.rs".to_string()));
    assert!(files.contains(&"/src/lib.rs".to_string()));
    assert!(files.contains(&"/notebooks/analysis.ipynb".to_string()));
    // Read/Bash files should NOT be included
    assert!(!files.contains(&"/src/other.rs".to_string()));
}

// =============================================================================
// Test 6: Goal context for project
// =============================================================================

#[tokio::test]
async fn test_goal_context_for_project() {
    let (pool, project_id) = setup_test_pool_with_project().await;

    db(&pool, move |conn| {
        seed_goal(conn, project_id, "Implement auth system", "in_progress", 30);
        seed_goal(conn, project_id, "Write documentation", "in_progress", 10);
        seed_goal(conn, project_id, "Fix bug #42", "planning", 0);
        Ok(())
    })
    .await;

    let goals = db(&pool, move |conn| {
        crate::db::get_active_goals_sync(conn, Some(project_id), 3).map_err(Into::into)
    })
    .await;

    assert_eq!(goals.len(), 3);
    let titles: Vec<&str> = goals.iter().map(|g| g.title.as_str()).collect();
    assert!(titles.contains(&"Implement auth system"));
    assert!(titles.contains(&"Write documentation"));
    assert!(titles.contains(&"Fix bug #42"));
}

// =============================================================================
// Test 7: Session snapshot roundtrip + build_working_on_summary
// =============================================================================

#[tokio::test]
async fn test_session_snapshot_roundtrip() {
    let (pool, project_id) = setup_test_pool_with_project().await;

    let snapshot_json = serde_json::json!({
        "tool_count": 20,
        "top_tools": [
            {"name": "Edit", "count": 12},
            {"name": "Read", "count": 6},
        ],
        "files_modified": ["/src/main.rs", "/src/lib.rs"],
    })
    .to_string();

    let snap_clone = snapshot_json.clone();
    db(&pool, move |conn| {
        seed_session(conn, "sess-snap", project_id, "completed");
        seed_session_snapshot(conn, "sess-snap", &snap_clone);
        Ok(())
    })
    .await;

    // Retrieve snapshot
    let retrieved = db(&pool, |conn| {
        Ok(super::session::get_session_snapshot_sync(conn, "sess-snap"))
    })
    .await;
    assert!(retrieved.is_some());

    // Parse and verify build_working_on_summary
    let snap: serde_json::Value = serde_json::from_str(&retrieved.unwrap()).unwrap();
    let summary = super::session::build_working_on_summary(&snap);
    assert!(summary.is_some());
    let summary_text = summary.unwrap();
    assert!(
        summary_text.contains("code editing"),
        "expected 'code editing', got: {}",
        summary_text
    );
    assert!(
        summary_text.contains("main.rs"),
        "expected 'main.rs', got: {}",
        summary_text
    );
}

// =============================================================================
// Test 8: Project isolation - sessions scoped to their project
// =============================================================================

#[tokio::test]
async fn test_project_isolation_sessions() {
    let (pool, project_a_id) = setup_test_pool_with_project().await;
    let project_b_id = setup_second_project(&pool).await;

    db(&pool, move |conn| {
        seed_session(conn, "sess-a1", project_a_id, "completed");
        seed_session(conn, "sess-a2", project_a_id, "completed");
        seed_session(conn, "sess-b1", project_b_id, "completed");
        Ok(())
    })
    .await;

    // Query sessions for project A
    let sessions_a = db(&pool, move |conn| {
        crate::db::get_recent_sessions_sync(conn, project_a_id, 10).map_err(Into::into)
    })
    .await;

    let session_ids_a: Vec<&str> = sessions_a.iter().map(|s| s.id.as_str()).collect();
    assert!(session_ids_a.contains(&"sess-a1"));
    assert!(session_ids_a.contains(&"sess-a2"));
    assert!(
        !session_ids_a.contains(&"sess-b1"),
        "project A sessions should NOT include project B's session"
    );

    // Query sessions for project B
    let sessions_b = db(&pool, move |conn| {
        crate::db::get_recent_sessions_sync(conn, project_b_id, 10).map_err(Into::into)
    })
    .await;

    let session_ids_b: Vec<&str> = sessions_b.iter().map(|s| s.id.as_str()).collect();
    assert!(session_ids_b.contains(&"sess-b1"));
    assert!(
        !session_ids_b.contains(&"sess-a1"),
        "project B sessions should NOT include project A's sessions"
    );
    assert_eq!(sessions_b.len(), 1);
}
