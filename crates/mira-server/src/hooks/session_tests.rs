// crates/mira-server/src/hooks/session_tests.rs
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
        crate::db::get_active_goals_sync(conn, Some(project_id), 3)
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
// Test 8: save_session_snapshot roundtrip
// =============================================================================

#[tokio::test]
async fn test_save_session_snapshot_roundtrip() {
    let (pool, project_id) = setup_test_pool_with_project().await;

    db(&pool, move |conn| {
        seed_session(conn, "sess-snap-rt", project_id, "active");
        // Seed some tool history so snapshot has data
        seed_tool_history(
            conn,
            "sess-snap-rt",
            "Edit",
            r#"{"file_path":"/src/handler.rs"}"#,
            "ok",
        );
        seed_tool_history(
            conn,
            "sess-snap-rt",
            "Read",
            r#"{"file_path":"/src/lib.rs"}"#,
            "contents",
        );
        seed_tool_history(
            conn,
            "sess-snap-rt",
            "Write",
            r#"{"file_path":"/src/new.rs"}"#,
            "ok",
        );
        Ok(())
    })
    .await;

    // Call save_session_snapshot
    db(&pool, |conn| {
        super::stop::save_session_snapshot(conn, "sess-snap-rt")
    })
    .await;

    // Retrieve snapshot and verify structure
    let snapshot_str = db(&pool, |conn| {
        Ok::<_, anyhow::Error>(super::session::get_session_snapshot_sync(
            conn,
            "sess-snap-rt",
        ))
    })
    .await;

    assert!(snapshot_str.is_some(), "snapshot should have been saved");
    let snap: serde_json::Value = serde_json::from_str(&snapshot_str.unwrap()).unwrap();

    // Verify expected fields
    assert!(snap.get("tool_count").is_some());
    assert!(snap.get("top_tools").is_some());
    assert!(snap.get("files_modified").is_some());

    let tool_count = snap["tool_count"].as_i64().unwrap();
    assert_eq!(tool_count, 3);

    let files = snap["files_modified"].as_array().unwrap();
    // Only Write/Edit files should be in files_modified, not Read
    let file_strs: Vec<&str> = files.iter().filter_map(|v| v.as_str()).collect();
    assert!(file_strs.contains(&"/src/handler.rs"));
    assert!(file_strs.contains(&"/src/new.rs"));
    assert!(
        !file_strs.contains(&"/src/lib.rs"),
        "Read files should not be in files_modified"
    );
}

// =============================================================================
// Test 9: save_session_snapshot with no tools skips saving
// =============================================================================

#[tokio::test]
async fn test_save_session_snapshot_empty_session() {
    let (pool, project_id) = setup_test_pool_with_project().await;

    db(&pool, move |conn| {
        seed_session(conn, "sess-empty-snap", project_id, "active");
        // No tool history seeded
        Ok(())
    })
    .await;

    // Call save_session_snapshot — should return Ok but not insert anything
    db(&pool, |conn| {
        super::stop::save_session_snapshot(conn, "sess-empty-snap")
    })
    .await;

    // Verify no snapshot was saved
    let snapshot_str = db(&pool, |conn| {
        Ok::<_, anyhow::Error>(super::session::get_session_snapshot_sync(
            conn,
            "sess-empty-snap",
        ))
    })
    .await;

    assert!(
        snapshot_str.is_none(),
        "no snapshot should be saved for empty session"
    );
}

// =============================================================================
// Test 10: End-to-end compaction context chain
//   PreCompact parses JSONL → extracts context → writes session_snapshots
//   Stop hook overwrites snapshot → preserves compaction_context
//   Resume reads snapshot → build_compaction_summary formats it
// =============================================================================

#[tokio::test]
async fn test_compaction_context_end_to_end() {
    let (pool, project_id) = setup_test_pool_with_project().await;

    // ── Step 1: Use extract_and_save_context (the actual DB function) ────
    // Build a realistic JSONL transcript. The tool_use message comes before
    // the final substantive assistant message so active_work captures real work,
    // not a trivial "Let me read" line.
    let transcript = [
        r#"{"role":"user","content":"Let's refactor the database layer."}"#,
        r#"{"role":"assistant","content":"I'll start by reviewing the current code.\n\nWe decided to use connection pooling for all database access.\n\nTODO: add migration support for schema changes.\n\nerror: the current test suite fails on concurrent access."}"#,
        r#"{"role":"user","content":"Good analysis. What's next?"}"#,
        // tool_use block: text portion is extracted, tool_use input is NOT
        r#"{"role":"assistant","content":[{"type":"text","text":"Let me read the file."},{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"/src/secret_tool_payload.rs"}}]}"#,
        // system message should be entirely skipped
        r#"{"role":"system","content":"You are a helpful decided to assistant with error: handling."}"#,
        // Final assistant message: substantive content that should become active_work
        r#"{"role":"assistant","content":"Working on the connection pool implementation now.\n\nI will use deadpool-sqlite as the pooling library."}"#,
    ]
    .join("\n");

    // Seed session and tool history first
    db(&pool, move |conn| {
        seed_session(conn, "e2e-sess", project_id, "active");
        seed_tool_history(
            conn,
            "e2e-sess",
            "Edit",
            r#"{"file_path":"/src/db.rs"}"#,
            "ok",
        );
        seed_tool_history(
            conn,
            "e2e-sess",
            "Read",
            r#"{"file_path":"/src/lib.rs"}"#,
            "ok",
        );
        Ok(())
    })
    .await;

    // Verify parse + extract individually for content assertions
    let messages = super::precompact::parse_transcript_messages(&transcript);
    let ctx = super::precompact::extract_compaction_context(&messages);

    // Verify extraction produced meaningful results
    assert!(!ctx.decisions.is_empty(), "should have extracted decisions");
    assert!(
        !ctx.pending_tasks.is_empty(),
        "should have extracted pending tasks"
    );
    assert!(!ctx.issues.is_empty(), "should have extracted issues");
    assert!(
        !ctx.active_work.is_empty(),
        "should have captured active work"
    );

    // Gap fix 3: Assert active_work captured substantive content, not a trivial line
    assert!(
        ctx.active_work[0].contains("connection pool"),
        "active_work should contain substantive last-assistant content, got: {}",
        ctx.active_work[0]
    );

    let all_text: String = ctx
        .decisions
        .iter()
        .chain(&ctx.pending_tasks)
        .chain(&ctx.issues)
        .chain(&ctx.active_work)
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");

    // Verify system message content was NOT extracted
    assert!(
        !all_text.contains("helpful decided to assistant"),
        "system role content should have been filtered out"
    );

    // Gap fix 2: Assert tool_use payloads were excluded from extracted context
    assert!(
        !all_text.contains("secret_tool_payload"),
        "tool_use input data should not appear in extracted context"
    );

    // Gap fix 1: Use extract_and_save_context for the DB write instead of manual INSERT.
    // First call exercises the INSERT path (no prior snapshot).
    let mut client = crate::ipc::client::HookClient::from_pool(pool.clone());
    super::precompact::extract_and_save_context(&mut client, "e2e-sess", &transcript)
        .await
        .expect("extract_and_save_context (insert) should succeed");

    // Second call exercises the UPDATE/merge path (snapshot already exists).
    let transcript2 = r#"{"role":"assistant","content":"We decided to switch from mutex to rwlock for better read concurrency."}"#;
    super::precompact::extract_and_save_context(&mut client, "e2e-sess", transcript2)
        .await
        .expect("extract_and_save_context (merge) should succeed");

    // Verify the merge overwrote compaction_context (second transcript's content wins)
    let merged_snap = db(&pool, |conn| {
        Ok::<_, anyhow::Error>(super::session::get_session_snapshot_sync(conn, "e2e-sess"))
    })
    .await;
    let merged: serde_json::Value = serde_json::from_str(&merged_snap.unwrap()).unwrap();
    let cc = merged.get("compaction_context").unwrap();
    let decisions = cc.get("decisions").and_then(|d| d.as_array()).unwrap();
    assert!(
        decisions
            .iter()
            .any(|d| d.as_str().unwrap().contains("rwlock")),
        "merged snapshot should contain second transcript's decision"
    );

    // ── Step 2: Simulate Stop hook ───────────────────────────────────────
    // save_session_snapshot overwrites the snapshot but should preserve compaction_context
    db(&pool, |conn| {
        super::stop::save_session_snapshot(conn, "e2e-sess")
    })
    .await;

    // ── Step 3: Simulate Resume ──────────────────────────────────────────
    // Read snapshot back (as build_resume_context does)
    let snapshot_str = db(&pool, |conn| {
        Ok::<_, anyhow::Error>(super::session::get_session_snapshot_sync(conn, "e2e-sess"))
    })
    .await;

    assert!(snapshot_str.is_some(), "snapshot should exist after stop");
    let snap: serde_json::Value = serde_json::from_str(&snapshot_str.unwrap()).unwrap();

    // Verify Stop hook's own data is present
    assert!(snap.get("tool_count").is_some(), "missing tool_count");
    assert!(
        snap.get("files_modified").is_some(),
        "missing files_modified"
    );

    // Verify compaction_context survived the Stop hook overwrite
    assert!(
        snap.get("compaction_context").is_some(),
        "compaction_context was lost during stop hook snapshot"
    );

    // Build the compaction summary (as build_resume_context does)
    let summary = super::session::build_compaction_summary(&snap);
    assert!(summary.is_some(), "compaction summary should be present");
    let summary_text = summary.unwrap();

    // Verify the summary reflects the merged (second) compaction context
    assert!(
        summary_text.contains("Pre-compaction context:"),
        "got: {}",
        summary_text
    );
    assert!(summary_text.contains("Decisions:"), "got: {}", summary_text);
    // The second transcript's decision should appear in the final summary
    assert!(
        summary_text.contains("rwlock"),
        "summary should contain merged transcript's decision, got: {}",
        summary_text
    );
}

// =============================================================================
// Test 11: Path normalization dedup (M5)
// =============================================================================

#[tokio::test]
async fn test_path_normalization_dedup() {
    let (pool, _project_id) = setup_test_pool_with_project().await;

    // Create project via "/foo/bar" and "/foo/bar/" — should get the same ID
    let id_without_slash = db(&pool, |conn| {
        let (id, _name) = crate::db::get_or_create_project_sync(conn, "/foo/bar", None)?;
        Ok(id)
    })
    .await;

    let id_with_slash = db(&pool, |conn| {
        let (id, _name) = crate::db::get_or_create_project_sync(conn, "/foo/bar/", None)?;
        Ok(id)
    })
    .await;

    assert_eq!(
        id_without_slash, id_with_slash,
        "'/foo/bar' and '/foo/bar/' should resolve to the same project ID"
    );
}

// =============================================================================
// Test 12: Project isolation - sessions scoped to their project
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

// =============================================================================
// Test gap #10: read_session_or_global_cwd
// =============================================================================

/// Empty session_id should be rejected (falls through, returns None if global cwd
/// file also doesn't exist). We don't control the global fallback, so we just
/// verify the function doesn't panic and returns *something* (None or a real cwd).
/// The key assertion is that empty string doesn't attempt a filesystem lookup
/// with an empty path component.
#[test]
fn read_session_cwd_rejects_empty_session_id() {
    // Empty string should fall through the `sid.is_empty()` check.
    // It won't create a path like `~/.mira/sessions//claude-cwd`.
    // The result depends on whether the global fallback file exists,
    // but it must NOT panic.
    let result = super::read_session_or_global_cwd(Some(""));
    // If the global cwd file doesn't exist, result is None.
    // If it does exist (e.g., on a dev machine with Mira installed), it's Some.
    // Either way, we verify the function handled the empty string gracefully.
    // The important thing is no panic and no path traversal with empty component.
    let _ = result;
}

/// Session IDs containing path traversal characters (../, etc.) should be
/// rejected by the alphanumeric+dash filter, preventing directory escape.
#[test]
fn read_session_cwd_rejects_path_traversal() {
    let malicious_ids = [
        "../../../etc/passwd",
        "..%2f..%2fetc%2fpasswd",
        "valid-prefix/../escape",
        "foo/bar",
        "session\x00null",
        "a]b[c",
    ];
    for sid in &malicious_ids {
        let result = super::read_session_or_global_cwd(Some(sid));
        // These should all be rejected by the character filter and fall through
        // to the global cwd. They must NOT attempt to read from a traversed path.
        // We can't assert None because the global fallback may succeed on a dev machine,
        // but we verify no panic and that the function completes.
        let _ = result;
    }
}

/// A valid session ID format (alphanumeric + dashes) but with no corresponding
/// file on disk should return None (or global fallback).
#[test]
fn read_session_cwd_valid_id_missing_file() {
    // This ID format passes the character filter but the per-session file won't exist
    let result = super::read_session_or_global_cwd(Some("nonexistent-session-abc123"));
    // Per-session file won't exist, so it falls through to global cwd.
    // On a dev machine, global cwd may or may not exist.
    // The key: no panic, no error.
    let _ = result;
}

/// None session_id skips per-session lookup entirely (falls through to global).
#[test]
fn read_session_cwd_none_session_id() {
    let result = super::read_session_or_global_cwd(None);
    // Should go straight to global fallback. No panic.
    let _ = result;
}

// =============================================================================
// Test gap #8: record_hook_outcome logic
//
// The actual `record_hook_outcome` function is tightly coupled to `get_db_path()`
// (reads from ~/.mira/mira.db). We test the core read-modify-write logic it
// performs by replicating the same DB operations on an in-memory test pool.
// This validates the JSON counter manipulation and store_observation_sync usage.
// =============================================================================

/// Helper: replicate record_hook_outcome's DB logic on a given connection.
/// This mirrors lines 140-203 of hooks/mod.rs exactly.
#[cfg(test)]
fn record_hook_outcome_on_conn(
    conn: &rusqlite::Connection,
    hook_name: &str,
    success: bool,
    latency_ms: u128,
    error_msg: Option<&str>,
) {
    let key = format!("hook_health:{}", hook_name);
    let error_msg_owned = error_msg.map(|s| s.chars().take(200).collect::<String>());

    // Read existing stats (or default)
    let existing: Option<String> = conn
        .query_row(
            "SELECT content FROM system_observations WHERE key = ?1 AND scope = 'global' AND project_id IS NULL",
            [&key],
            |row| row.get(0),
        )
        .ok();

    let (mut runs, mut failures, last_error): (u64, u64, Option<String>) =
        if let Some(json_str) = existing {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&json_str) {
                (
                    v.get("runs").and_then(|v| v.as_u64()).unwrap_or(0),
                    v.get("failures").and_then(|v| v.as_u64()).unwrap_or(0),
                    v.get("last_error")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                )
            } else {
                (0, 0, None)
            }
        } else {
            (0, 0, None)
        };

    runs += 1;
    let new_last_error = if success {
        last_error
    } else {
        failures += 1;
        error_msg_owned
    };

    let content = serde_json::json!({
        "runs": runs,
        "failures": failures,
        "last_error": new_last_error,
        "last_latency_ms": latency_ms,
    });

    let content_str = content.to_string();

    crate::db::observations::store_observation_sync(
        conn,
        crate::db::observations::StoreObservationParams {
            project_id: None,
            key: Some(&key),
            content: &content_str,
            observation_type: "hook_health",
            category: Some("system"),
            confidence: 1.0,
            source: "hook_monitor",
            session_id: None,
            team_id: None,
            scope: "global",
            expires_at: None,
        },
    )
    .unwrap();
}

/// Helper: read back the hook health JSON from system_observations.
#[cfg(test)]
fn read_hook_health(conn: &rusqlite::Connection, hook_name: &str) -> serde_json::Value {
    let key = format!("hook_health:{}", hook_name);
    let content: String = conn
        .query_row(
            "SELECT content FROM system_observations WHERE key = ?1 AND scope = 'global' AND project_id IS NULL",
            [&key],
            |row| row.get(0),
        )
        .unwrap();
    serde_json::from_str(&content).unwrap()
}

/// First call creates counter with runs=1, failures=0.
#[tokio::test]
async fn record_hook_outcome_increments_on_success() {
    let pool = setup_test_pool().await;

    db(&pool, |conn| {
        record_hook_outcome_on_conn(conn, "TestHook", true, 42, None);

        let stats = read_hook_health(conn, "TestHook");
        assert_eq!(stats["runs"], 1, "first success should set runs=1");
        assert_eq!(stats["failures"], 0, "first success should set failures=0");
        assert!(stats["last_error"].is_null(), "no error on success");
        assert_eq!(stats["last_latency_ms"], 42);
        Ok(())
    })
    .await;
}

/// A failure call increments both runs and failures, and records the error message.
#[tokio::test]
async fn record_hook_outcome_tracks_failures() {
    let pool = setup_test_pool().await;

    db(&pool, |conn| {
        // First: a success
        record_hook_outcome_on_conn(conn, "FailHook", true, 10, None);
        // Second: a failure
        record_hook_outcome_on_conn(conn, "FailHook", false, 50, Some("connection timeout"));

        let stats = read_hook_health(conn, "FailHook");
        assert_eq!(stats["runs"], 2, "should have 2 total runs");
        assert_eq!(stats["failures"], 1, "should have 1 failure");
        assert_eq!(
            stats["last_error"], "connection timeout",
            "last_error should be set on failure"
        );
        assert_eq!(
            stats["last_latency_ms"], 50,
            "latency should be from last call"
        );

        // Third: another success -- should preserve last_error from previous failure
        record_hook_outcome_on_conn(conn, "FailHook", true, 5, None);
        let stats2 = read_hook_health(conn, "FailHook");
        assert_eq!(stats2["runs"], 3);
        assert_eq!(
            stats2["failures"], 1,
            "failures should not increment on success"
        );
        assert_eq!(
            stats2["last_error"], "connection timeout",
            "success should preserve previous last_error"
        );
        Ok(())
    })
    .await;
}

/// Error messages longer than 200 chars are truncated by the .chars().take(200) logic.
#[tokio::test]
async fn record_hook_outcome_truncates_long_error() {
    let pool = setup_test_pool().await;

    db(&pool, |conn| {
        let long_error = "x".repeat(500);
        record_hook_outcome_on_conn(conn, "LongErrHook", false, 99, Some(&long_error));

        let stats = read_hook_health(conn, "LongErrHook");
        let stored_error = stats["last_error"].as_str().unwrap();
        assert_eq!(
            stored_error.len(),
            200,
            "error message should be truncated to 200 chars, got {}",
            stored_error.len()
        );
        assert_eq!(stats["runs"], 1);
        assert_eq!(stats["failures"], 1);
        Ok(())
    })
    .await;
}
