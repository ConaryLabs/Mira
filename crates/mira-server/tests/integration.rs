//! Integration tests for Mira MCP tools
//!
//! These tests verify the integration between tool functions and their dependencies,
//! using mocked or in-memory implementations where appropriate.

mod test_utils;

use mira::mcp::requests::{GoalAction, GoalRequest, IndexAction};
use mira::mcp::responses::*;
use mira::tools::core::{
    ToolContext, ensure_session, find_function_callees, find_function_callers, get_project,
    get_session_recap, get_symbols, goal, handle_launch, handle_session, index, search_code,
    session_start, set_project, summarize_codebase,
};
use mira::tools::tasks::TaskAction;
use std::sync::Arc;
use test_utils::TestContext;

/// Extract message text from Json<T> output for test assertions
macro_rules! msg {
    ($output:expr) => {
        $output.0.message.as_str()
    };
}

#[tokio::test]
async fn test_session_start_basic() {
    let ctx = TestContext::new().await;

    // Test session_start with a project path
    let project_path = "/tmp/test_project".to_string();
    let project_name = Some("Test Project".to_string());

    let result = session_start(&ctx, project_path.clone(), project_name.clone(), None).await;

    // Check that session_start succeeded
    assert!(result.is_ok(), "session_start failed: {:?}", result.err());

    let output = result.unwrap();

    // Verify output contains expected information
    assert!(
        msg!(output).contains("Project:"),
        "Output should contain project info"
    );
    assert!(
        msg!(output).contains("Test Project"),
        "Output should contain project name"
    );
    assert!(
        msg!(output).contains("Ready."),
        "Output should end with Ready."
    );

    // Verify project was set in context
    let project = ctx.get_project().await;
    assert!(project.is_some(), "Project should be set in context");
    let project = project.unwrap();
    assert_eq!(project.path, project_path);
    assert_eq!(project.name, project_name);

    // Verify session ID was set
    let session_id = ctx.get_session_id().await;
    assert!(session_id.is_some(), "Session ID should be set");
}

#[tokio::test]
async fn test_set_project_get_project() {
    let ctx = TestContext::new().await;

    // Test set_project
    let project_path = "/tmp/another_project".to_string();
    let project_name = Some("Another Project".to_string());

    let set_result = set_project(&ctx, project_path.clone(), project_name.clone()).await;
    assert!(
        set_result.is_ok(),
        "set_project failed: {:?}",
        set_result.err()
    );

    // Test get_project
    let get_result = get_project(&ctx).await;
    assert!(
        get_result.is_ok(),
        "get_project failed: {:?}",
        get_result.err()
    );

    let output = get_result.unwrap();
    assert!(
        msg!(output).contains("Current project:"),
        "Output should indicate current project"
    );
    assert!(
        msg!(output).contains("/tmp/another_project"),
        "Output should contain project path"
    );
    assert!(
        msg!(output).contains("Another Project"),
        "Output should contain project name"
    );

    // Verify project context
    let project = ctx.get_project().await;
    assert!(project.is_some(), "Project should be set");
    let project = project.unwrap();
    assert_eq!(project.path, project_path);
    assert_eq!(project.name, project_name);
}

#[tokio::test]
async fn test_session_start_with_existing_session_id() {
    let ctx = TestContext::new().await;

    // Provide a custom session ID
    let custom_session_id = "test-session-123".to_string();
    let project_path = "/tmp/test_custom_session".to_string();

    let result = session_start(&ctx, project_path, None, Some(custom_session_id.clone())).await;

    assert!(
        result.is_ok(),
        "session_start with custom ID failed: {:?}",
        result.err()
    );

    // Verify the custom session ID was used
    let session_id = ctx.get_session_id().await;
    assert_eq!(session_id, Some(custom_session_id));
}

#[tokio::test]
async fn test_session_start_twice_different_projects() {
    let ctx = TestContext::new().await;

    // First session_start
    let result1 = session_start(
        &ctx,
        "/tmp/project1".to_string(),
        Some("Project 1".to_string()),
        None,
    )
    .await;
    assert!(result1.is_ok(), "First session_start failed");

    let project1 = ctx.get_project().await.unwrap();
    let session_id1 = ctx.get_session_id().await.unwrap();

    // Second session_start with different project
    let result2 = session_start(
        &ctx,
        "/tmp/project2".to_string(),
        Some("Project 2".to_string()),
        None,
    )
    .await;
    assert!(result2.is_ok(), "Second session_start failed");

    let project2 = ctx.get_project().await.unwrap();
    let session_id2 = ctx.get_session_id().await.unwrap();

    // Verify project changed
    assert_ne!(project1.path, project2.path);
    assert_ne!(project1.name, project2.name);

    // Verify session ID changed (new session for new project)
    assert_ne!(session_id1, session_id2);
}

#[tokio::test]
async fn test_search_code_empty() {
    let ctx = TestContext::new().await;

    let project_path = "/tmp/test_code_search".to_string();
    session_start(
        &ctx,
        project_path.clone(),
        Some("Code Search Test".to_string()),
        None,
    )
    .await
    .expect("session_start failed");

    let result = search_code(&ctx, "function foo".to_string(), Some(10)).await;
    assert!(result.is_ok(), "search_code failed: {:?}", result.err());
    let output = result.unwrap();
    assert!(
        msg!(output).contains("No code index found")
            || msg!(output).contains("No code matches found"),
        "Output: {}",
        msg!(output)
    );
}

#[tokio::test]
async fn test_find_function_callers_empty() {
    let ctx = TestContext::new().await;

    let project_path = "/tmp/test_callers".to_string();
    session_start(
        &ctx,
        project_path.clone(),
        Some("Callers Test".to_string()),
        None,
    )
    .await
    .expect("session_start failed");

    let result = find_function_callers(&ctx, "some_function".to_string(), Some(20)).await;
    assert!(
        result.is_ok(),
        "find_function_callers failed: {:?}",
        result.err()
    );
    let output = result.unwrap();
    assert!(
        msg!(output).contains("No callers found"),
        "Output: {}",
        msg!(output)
    );
}

#[tokio::test]
async fn test_find_function_callees_empty() {
    let ctx = TestContext::new().await;

    let project_path = "/tmp/test_callees".to_string();
    session_start(
        &ctx,
        project_path.clone(),
        Some("Callees Test".to_string()),
        None,
    )
    .await
    .expect("session_start failed");

    let result = find_function_callees(&ctx, "some_function".to_string(), Some(20)).await;
    assert!(
        result.is_ok(),
        "find_function_callees failed: {:?}",
        result.err()
    );
    let output = result.unwrap();
    assert!(
        msg!(output).contains("No callees found"),
        "Output: {}",
        msg!(output)
    );
}

#[tokio::test]
async fn test_index_status() {
    let ctx = TestContext::new().await;

    let project_path = "/tmp/test_index".to_string();
    session_start(
        &ctx,
        project_path.clone(),
        Some("Index Test".to_string()),
        None,
    )
    .await
    .expect("session_start failed");

    let result = index(&ctx, IndexAction::Status, None, false).await;
    assert!(result.is_ok(), "index status failed: {:?}", result.err());
    let output = result.unwrap();
    assert!(
        msg!(output).contains("Index status"),
        "Output: {}",
        msg!(output)
    );
    assert!(
        msg!(output).contains("symbols") && msg!(output).contains("embedded chunks"),
        "Output: {}",
        msg!(output)
    );
}

#[tokio::test]
async fn test_get_symbols() {
    use std::fs;

    // Create a temporary Rust file
    let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
    let file_path = format!("{}/test.rs", temp_dir.path().display());
    let content = r#"
// A simple Rust module
fn hello_world() {
    println!("Hello, world!");
}

struct Point {
    x: i32,
    y: i32,
}

impl Point {
    fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}
"#;
    fs::write(&file_path, content).expect("Failed to write test file");

    // Call get_symbols
    let result = get_symbols(file_path.clone(), None);
    assert!(result.is_ok(), "get_symbols failed: {:?}", result.err());
    let output = result.unwrap();
    assert!(msg!(output).contains("symbols"), "Output: {}", msg!(output));
    // Should contain function and struct
    assert!(
        msg!(output).contains("hello_world") || msg!(output).contains("Point"),
        "Output: {}",
        msg!(output)
    );

    // temp_dir is cleaned up automatically when dropped
}

#[tokio::test]
async fn test_summarize_codebase_no_deepseek() {
    let ctx = TestContext::new().await;

    let project_path = "/tmp/test_summarize".to_string();
    session_start(
        &ctx,
        project_path.clone(),
        Some("Summarize Test".to_string()),
        None,
    )
    .await
    .expect("session_start failed");

    let result = summarize_codebase(&ctx).await;
    // Without LLM, either succeeds with heuristic fallback or returns "All modules already have summaries"
    assert!(
        result.is_ok(),
        "summarize_codebase should succeed with heuristic fallback: {:?}",
        result.err()
    );
    let output = result.unwrap();
    assert!(
        msg!(output).contains("Summarized")
            || msg!(output).contains("All modules already have summaries"),
        "Output: {}",
        msg!(output)
    );
}

#[tokio::test]
async fn test_ensure_session() {
    let ctx = TestContext::new().await;

    // No session initially
    let session_id = ctx.get_session_id().await;
    assert!(session_id.is_none());

    // Call ensure_session
    let result = ensure_session(&ctx).await;
    assert!(result.is_ok(), "ensure_session failed: {:?}", result.err());
    let new_session_id = result.unwrap();
    assert!(!new_session_id.is_empty());

    // Verify session is set in context
    let ctx_session_id = ctx.get_session_id().await;
    assert_eq!(ctx_session_id, Some(new_session_id));
}

#[tokio::test]
async fn test_session_history_current() {
    use mira::mcp::requests::{SessionAction, SessionRequest};
    let ctx = TestContext::new().await;

    // No active session
    let req = SessionRequest {
        action: SessionAction::CurrentSession,
        session_id: None,

        limit: None,
        group_by: None,
        since_days: None,
        insight_source: None,
        min_confidence: None,
        insight_id: None,
        dry_run: None,
        category: None,
    };
    let result = handle_session(&ctx, req).await;
    assert!(
        result.is_ok(),
        "session current_session failed: {:?}",
        result.err()
    );
    let output = result.unwrap();
    assert!(
        msg!(output).contains("No active session"),
        "Output: {}",
        msg!(output)
    );

    // Create a session via session_start
    let project_path = "/tmp/test_session_history".to_string();
    session_start(
        &ctx,
        project_path.clone(),
        Some("Session History Test".to_string()),
        None,
    )
    .await
    .expect("session_start failed");

    let req = SessionRequest {
        action: SessionAction::CurrentSession,
        session_id: None,

        limit: None,
        group_by: None,
        since_days: None,
        insight_source: None,
        min_confidence: None,
        insight_id: None,
        dry_run: None,
        category: None,
    };
    let result = handle_session(&ctx, req).await;
    assert!(
        result.is_ok(),
        "session current_session failed: {:?}",
        result.err()
    );
    let output = result.unwrap();
    assert!(
        msg!(output).contains("Current session:"),
        "Output: {}",
        msg!(output)
    );
}

#[tokio::test]
async fn test_session_history_list_sessions() {
    use mira::mcp::requests::{SessionAction, SessionRequest};
    let ctx = TestContext::new().await;

    let project_path = "/tmp/test_list_sessions".to_string();
    session_start(
        &ctx,
        project_path.clone(),
        Some("List Sessions Test".to_string()),
        None,
    )
    .await
    .expect("session_start failed");

    let req = SessionRequest {
        action: SessionAction::ListSessions,
        session_id: None,

        limit: Some(10),
        group_by: None,
        since_days: None,
        insight_source: None,
        min_confidence: None,
        insight_id: None,
        dry_run: None,
        category: None,
    };
    let result = handle_session(&ctx, req).await;
    // Should succeed even if no sessions in database (maybe there is one now)
    assert!(
        result.is_ok(),
        "session list_sessions failed: {:?}",
        result.err()
    );
    let output = result.unwrap();
    // Output either lists sessions or says "No sessions found"
    assert!(
        msg!(output).contains("sessions") || msg!(output).contains("No sessions"),
        "Output: {}",
        msg!(output)
    );
}

#[tokio::test]
async fn test_goal_create_and_list() {
    let ctx = TestContext::new().await;

    let project_path = "/tmp/test_goals".to_string();
    session_start(
        &ctx,
        project_path.clone(),
        Some("Goal Test".to_string()),
        None,
    )
    .await
    .expect("session_start failed");

    // Create a goal
    let result = goal(
        &ctx,
        GoalRequest {
            action: GoalAction::Create,
            goal_id: None,
            title: Some("Implement new feature".to_string()),
            description: Some("Add user authentication".to_string()),
            status: Some("planning".to_string()),
            priority: Some("high".to_string()),
            progress_percent: Some(0),
            include_finished: None,
            milestone_id: None,
            milestone_title: None,
            weight: None,
            limit: None,
            goals: None,
        },
    )
    .await;
    assert!(result.is_ok(), "goal create failed: {:?}", result.err());
    let output = result.unwrap();
    assert!(
        msg!(output).contains("Created goal"),
        "Output: {}",
        msg!(output)
    );
    assert!(
        msg!(output).contains("Implement new feature"),
        "Output: {}",
        msg!(output)
    );

    // List goals
    let result = goal(
        &ctx,
        GoalRequest {
            action: GoalAction::List,
            goal_id: None,
            title: None,
            description: None,
            status: None,
            priority: None,
            progress_percent: None,
            include_finished: Some(false),
            milestone_id: None,
            milestone_title: None,
            weight: None,
            limit: Some(10),
            goals: None,
        },
    )
    .await;
    assert!(result.is_ok(), "goal list failed: {:?}", result.err());
    let output = result.unwrap();
    assert!(msg!(output).contains("goals"), "Output: {}", msg!(output));
    assert!(
        msg!(output).contains("Implement new feature"),
        "Output: {}",
        msg!(output)
    );
}

#[tokio::test]
async fn test_goal_list_limit_zero_shows_total() {
    let ctx = TestContext::new().await;

    let project_path = "/tmp/test_goal_limit0".to_string();
    session_start(
        &ctx,
        project_path.clone(),
        Some("Limit Zero Test".to_string()),
        None,
    )
    .await
    .expect("session_start failed");

    // Create a goal so total > 0
    goal(
        &ctx,
        GoalRequest {
            action: GoalAction::Create,
            goal_id: None,
            title: Some("Test goal".to_string()),
            description: None,
            status: None,
            priority: None,
            progress_percent: None,
            include_finished: None,
            milestone_id: None,
            milestone_title: None,
            weight: None,
            limit: None,
            goals: None,
        },
    )
    .await
    .expect("goal create failed");

    // List with limit=0: should report total but show no items
    let result = goal(
        &ctx,
        GoalRequest {
            action: GoalAction::List,
            goal_id: None,
            title: None,
            description: None,
            status: None,
            priority: None,
            progress_percent: None,
            include_finished: Some(false),
            milestone_id: None,
            milestone_title: None,
            weight: None,
            limit: Some(0),
            goals: None,
        },
    )
    .await;
    assert!(result.is_ok(), "goal list failed: {:?}", result.err());
    let output = result.unwrap();
    let msg = msg!(output);
    // Should NOT say "No goals found" since goals exist
    assert!(
        !msg.contains("No goals found"),
        "limit=0 should not say 'No goals found' when goals exist: {}",
        msg
    );
    // Should report the real total
    assert!(
        msg.contains("(showing 0)"),
        "limit=0 should show '(showing 0)': {}",
        msg
    );
}

#[tokio::test]
async fn test_get_session_recap() {
    let ctx = TestContext::new().await;

    let project_path = "/tmp/test_recap".to_string();
    session_start(
        &ctx,
        project_path.clone(),
        Some("Recap Test".to_string()),
        None,
    )
    .await
    .expect("session_start failed");

    let result = get_session_recap(&ctx).await;
    // Should succeed, may return "No session recap available."
    assert!(
        result.is_ok(),
        "get_session_recap failed: {:?}",
        result.err()
    );
    let output = result.unwrap();
    // Should contain project name or say no recap available
    assert!(
        output.contains("Recap Test") || output.contains("No session recap"),
        "Output: {}",
        output
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Pool Behavior Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_pool_concurrent_access() {
    use mira::tools::core::ToolContext;

    let ctx = TestContext::new().await;

    // Set up a project first
    let project_path = "/tmp/test_pool_concurrent".to_string();
    session_start(
        &ctx,
        project_path.clone(),
        Some("Pool Test".to_string()),
        None,
    )
    .await
    .expect("session_start failed");

    let project_id = ctx.project_id().await.expect("Should have project_id");

    // Run multiple concurrent goal operations to verify pool handles concurrency.
    // Stagger starts to avoid thundering herd on in-memory shared-cache DB.
    let futures: Vec<_> = (0..5)
        .map(|i| {
            let ctx_ref = &ctx;
            async move {
                tokio::time::sleep(std::time::Duration::from_millis(i * 50)).await;
                goal(
                    ctx_ref,
                    GoalRequest {
                        action: GoalAction::Create,
                        title: Some(format!("Concurrent goal {}", i)),
                        description: None,
                        status: None,
                        priority: None,
                        progress_percent: None,
                        include_finished: None,
                        goal_id: None,
                        milestone_id: None,
                        milestone_title: None,
                        weight: None,
                        limit: None,
                        goals: None,
                    },
                )
                .await
            }
        })
        .collect();

    let results = futures::future::join_all(futures).await;

    // All should succeed
    for (i, result) in results.iter().enumerate() {
        assert!(
            result.is_ok(),
            "Concurrent goal create {} failed: {:?}",
            i,
            result.as_ref().err()
        );
    }

    // Verify all goals were stored
    let count: i64 = ctx
        .pool()
        .interact(move |conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM goals WHERE project_id = ?1",
                [project_id],
                |row| row.get(0),
            )
            .map_err(|e| anyhow::anyhow!(e))
        })
        .await
        .unwrap();

    assert!(count >= 5, "Should have at least 5 goals, got {}", count);
}

#[tokio::test]
async fn test_pool_and_database_share_state() {
    use mira::tools::core::ToolContext;

    let ctx = TestContext::new().await;

    // Create a project using pool (via session_start)
    let project_path = "/tmp/test_pool_share".to_string();
    session_start(
        &ctx,
        project_path.clone(),
        Some("Share Test".to_string()),
        None,
    )
    .await
    .expect("session_start failed");

    let project_id = ctx.project_id().await.expect("Should have project_id");

    // Verify project exists via pool
    let project_exists = ctx
        .pool()
        .interact(move |conn| {
            Ok::<bool, anyhow::Error>(
                conn.query_row(
                    "SELECT 1 FROM projects WHERE id = ?",
                    [project_id],
                    |_row| Ok(true),
                )
                .unwrap_or(false),
            )
        })
        .await
        .unwrap();

    assert!(project_exists, "Project created via pool should be visible");

    // Create a goal via pool
    goal(
        &ctx,
        GoalRequest {
            action: GoalAction::Create,
            title: Some("Pool-created goal".to_string()),
            description: None,
            status: None,
            priority: None,
            progress_percent: None,
            include_finished: None,
            goal_id: None,
            milestone_id: None,
            milestone_title: None,
            weight: None,
            limit: None,
            goals: None,
        },
    )
    .await
    .expect("goal create failed");

    // Verify goal exists via pool
    let goal_exists = ctx
        .pool()
        .interact(move |conn| {
            Ok::<bool, anyhow::Error>(
                conn.query_row(
                    "SELECT 1 FROM goals WHERE project_id = ?",
                    [project_id],
                    |_row| Ok(true),
                )
                .unwrap_or(false),
            )
        })
        .await
        .unwrap();

    assert!(goal_exists, "Goal created via pool should be visible");
}

// ═══════════════════════════════════════════════════════════════════════════════
// Context Injection Integration Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_context_injection_basic() {
    use mira::context::ContextInjectionManager;

    let ctx = TestContext::new().await;

    // Create injection manager
    let manager =
        ContextInjectionManager::new(ctx.pool().inner().clone(), None, ctx.embeddings().cloned(), None)
            .await;

    // Test with a code-related message
    let result = manager
        .get_context_for_message(
            "How does the authentication function work in this codebase?",
            "test-session",
        )
        .await;

    // Should attempt injection (may or may not find context depending on DB state)
    assert!(
        result.skip_reason.is_none() || result.skip_reason == Some("sampled_out".to_string()),
        "Should not skip for code-related message, got: {:?}",
        result.skip_reason
    );
}

#[tokio::test]
async fn test_context_injection_skip_simple_commands() {
    use mira::context::ContextInjectionManager;

    let ctx = TestContext::new().await;
    let manager =
        ContextInjectionManager::new(ctx.pool().inner().clone(), None, ctx.embeddings().cloned(), None)
            .await;

    // Simple commands should be skipped
    let result = manager
        .get_context_for_message("git status", "test-session")
        .await;
    assert_eq!(result.skip_reason, Some("simple_command".to_string()));

    let result = manager
        .get_context_for_message("ls -la", "test-session")
        .await;
    assert_eq!(result.skip_reason, Some("simple_command".to_string()));

    let result = manager
        .get_context_for_message("/help", "test-session")
        .await;
    assert_eq!(result.skip_reason, Some("simple_command".to_string()));
}

#[tokio::test]
async fn test_context_injection_skip_short_messages() {
    use mira::context::ContextInjectionManager;

    let ctx = TestContext::new().await;
    let manager =
        ContextInjectionManager::new(ctx.pool().inner().clone(), None, ctx.embeddings().cloned(), None)
            .await;

    // Very short messages should be skipped
    let result = manager.get_context_for_message("hi", "test-session").await;
    assert!(result.skip_reason.is_some());
}

#[tokio::test]
async fn test_context_injection_config() {
    use mira::context::{ContextInjectionManager, InjectionConfig};

    let ctx = TestContext::new().await;
    let mut manager =
        ContextInjectionManager::new(ctx.pool().inner().clone(), None, ctx.embeddings().cloned(), None)
            .await;

    // Verify default config
    assert!(manager.config().enabled);
    assert_eq!(manager.config().max_chars, 3000);
    assert_eq!(manager.config().sample_rate, 1.0);

    // Update config
    let new_config = InjectionConfig::builder()
        .enabled(false)
        .max_chars(2000)
        .sample_rate(1.0)
        .build();
    manager.set_config(new_config).await;

    // Verify injection is disabled
    let result = manager
        .get_context_for_message("How does the authentication function work?", "test-session")
        .await;
    assert_eq!(result.skip_reason, Some("disabled".to_string()));
}

#[tokio::test]
async fn test_context_injection_with_goals() {
    use mira::context::ContextInjectionManager;

    let ctx = TestContext::new().await;

    // Create a project and some goals
    let project_path = "/tmp/test_injection_goals".to_string();
    session_start(
        &ctx,
        project_path.clone(),
        Some("Injection Test".to_string()),
        None,
    )
    .await
    .expect("session_start failed");

    // Create a goal
    goal(
        &ctx,
        GoalRequest {
            action: GoalAction::Create,
            goal_id: None,
            title: Some("Fix authentication bug".to_string()),
            description: Some("High priority security issue".to_string()),
            status: None,
            priority: Some("high".to_string()),
            progress_percent: None,
            include_finished: None,
            milestone_id: None,
            milestone_title: None,
            weight: None,
            limit: None,
            goals: None,
        },
    )
    .await
    .expect("goal creation failed");

    // Create injection manager
    let manager =
        ContextInjectionManager::new(ctx.pool().inner().clone(), None, ctx.embeddings().cloned(), None)
            .await;

    // Get context - should include goal info if task-aware injection is enabled
    // Note: due to sampling, this might be skipped
    let config = manager.config();
    assert!(
        config.enable_task_aware,
        "Task-aware injection should be enabled by default"
    );
}

#[tokio::test]
async fn test_context_injection_file_extraction() {
    use mira::context::FileAwareInjector;

    let ctx = TestContext::new().await;
    let injector = FileAwareInjector::new(ctx.pool().inner().clone());

    // Test file path extraction
    let paths = injector.extract_file_mentions("Check src/main.rs and lib.rs for issues");
    assert!(paths.contains(&"src/main.rs".to_string()));
    assert!(paths.contains(&"lib.rs".to_string()));

    // Test with nested paths
    let paths = injector.extract_file_mentions("Look at crates/mira-server/src/db/pool.rs");
    assert!(paths.contains(&"crates/mira-server/src/db/pool.rs".to_string()));

    // Test with various extensions
    let paths = injector.extract_file_mentions("Edit config.toml and package.json");
    assert!(paths.contains(&"config.toml".to_string()));
    assert!(paths.contains(&"package.json".to_string()));
}

// ═══════════════════════════════════════════════════════════════════════════════
// Documentation System Integration Tests
// ═══════════════════════════════════════════════════════════════════════════════

use mira::mcp::requests::{DocumentationAction, DocumentationRequest};
use mira::tools::core::documentation;

#[tokio::test]
async fn test_documentation_list_empty() {
    let ctx = TestContext::new().await;

    let project_path = "/tmp/test_doc_list".to_string();
    session_start(
        &ctx,
        project_path.clone(),
        Some("Doc List Test".to_string()),
        None,
    )
    .await
    .expect("session_start failed");

    // List documentation tasks - should be empty
    let result = documentation(
        &ctx,
        DocumentationRequest {
            action: DocumentationAction::List,
            task_id: None,
            task_ids: None,
            reason: None,
            doc_type: None,
            priority: None,
            status: None,
            limit: None,
            offset: None,
        },
    )
    .await;

    assert!(
        result.is_ok(),
        "documentation list failed: {:?}",
        result.err()
    );
    let output = result.unwrap();
    assert!(
        msg!(output).contains("No documentation tasks found"),
        "Output: {}",
        msg!(output)
    );
}

#[tokio::test]
async fn test_documentation_list_requires_project() {
    let ctx = TestContext::new().await;

    // No project set - should error
    let result = documentation(
        &ctx,
        DocumentationRequest {
            action: DocumentationAction::List,
            task_id: None,
            task_ids: None,
            reason: None,
            doc_type: None,
            priority: None,
            status: None,
            limit: None,
            offset: None,
        },
    )
    .await;

    assert!(result.is_err(), "Should fail without active project");
    let error = result.err().expect("should be Err");
    assert!(
        error.to_string().contains("No active project"),
        "Error should mention no active project: {}",
        error
    );
}

#[tokio::test]
async fn test_documentation_list_with_tasks() {
    use mira::tools::core::ToolContext;

    let ctx = TestContext::new().await;

    let project_path = "/tmp/test_doc_list_tasks".to_string();
    session_start(
        &ctx,
        project_path.clone(),
        Some("Doc List Tasks Test".to_string()),
        None,
    )
    .await
    .expect("session_start failed");

    let project_id = ctx.project_id().await.expect("Should have project_id");

    // Create a doc task directly in the database
    ctx.pool()
        .run(move |conn| {
            conn.execute(
                "INSERT INTO documentation_tasks (
                    project_id, doc_type, doc_category, source_file_path,
                    target_doc_path, priority, status, reason
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    project_id,
                    "mcp_tool",
                    "mcp_tool",
                    "src/tools/example.rs",
                    "docs/tools/example.md",
                    "high",
                    "pending",
                    "Missing documentation for public API"
                ],
            )
        })
        .await
        .expect("Failed to create doc task");

    // List documentation tasks
    let result = documentation(
        &ctx,
        DocumentationRequest {
            action: DocumentationAction::List,
            task_id: None,
            task_ids: None,
            reason: None,
            doc_type: None,
            priority: None,
            status: None,
            limit: None,
            offset: None,
        },
    )
    .await;

    assert!(
        result.is_ok(),
        "documentation list failed: {:?}",
        result.err()
    );
    let output = result.unwrap();
    assert!(
        msg!(output).contains("Documentation Tasks"),
        "Output should contain header: {}",
        msg!(output)
    );
    assert!(
        msg!(output).contains("docs/tools/example.md"),
        "Output should contain task path: {}",
        msg!(output)
    );
    assert!(
        msg!(output).contains("high"),
        "Output should contain priority: {}",
        msg!(output)
    );
}

#[tokio::test]
async fn test_documentation_get_task_details() {
    use mira::tools::core::ToolContext;

    let ctx = TestContext::new().await;

    let project_path = "/tmp/test_doc_get".to_string();
    session_start(
        &ctx,
        project_path.clone(),
        Some("Doc Get Test".to_string()),
        None,
    )
    .await
    .expect("session_start failed");

    let project_id = ctx.project_id().await.expect("Should have project_id");

    // Create a doc task
    let task_id: i64 = ctx
        .pool()
        .run(move |conn| {
            conn.execute(
                "INSERT INTO documentation_tasks (
                    project_id, doc_type, doc_category, source_file_path,
                    target_doc_path, priority, status, reason
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    project_id,
                    "mcp_tool",
                    "mcp_tool",
                    "src/tools/auth.rs",
                    "docs/tools/auth.md",
                    "high",
                    "pending",
                    "New tool needs documentation"
                ],
            )?;
            Ok::<i64, rusqlite::Error>(conn.last_insert_rowid())
        })
        .await
        .expect("Failed to create doc task");

    // Get task details
    let result = documentation(
        &ctx,
        DocumentationRequest {
            action: DocumentationAction::Get,
            task_id: Some(task_id),
            task_ids: None,
            reason: None,
            doc_type: None,
            priority: None,
            status: None,
            limit: None,
            offset: None,
        },
    )
    .await;

    assert!(
        result.is_ok(),
        "documentation get failed: {:?}",
        result.err()
    );
    let output = result.unwrap();
    assert!(
        msg!(output).contains("Documentation Task"),
        "Output should contain header: {}",
        msg!(output)
    );
    assert!(
        msg!(output).contains("docs/tools/auth.md"),
        "Output should contain target path: {}",
        msg!(output)
    );
    assert!(
        msg!(output).contains("src/tools/auth.rs"),
        "Output should contain source path: {}",
        msg!(output)
    );
    assert!(
        msg!(output).contains("Writing Guidelines"),
        "Output should contain guidelines: {}",
        msg!(output)
    );
    assert!(
        msg!(output).contains("MCP tool documentation"),
        "Output should have MCP-specific guidelines: {}",
        msg!(output)
    );
}

#[tokio::test]
async fn test_documentation_get_requires_task_id() {
    let ctx = TestContext::new().await;

    let project_path = "/tmp/test_doc_get_no_id".to_string();
    session_start(
        &ctx,
        project_path.clone(),
        Some("Doc Get No ID Test".to_string()),
        None,
    )
    .await
    .expect("session_start failed");

    // Get without task_id should fail
    let result = documentation(
        &ctx,
        DocumentationRequest {
            action: DocumentationAction::Get,
            task_id: None,
            task_ids: None,
            reason: None,
            doc_type: None,
            priority: None,
            status: None,
            limit: None,
            offset: None,
        },
    )
    .await;

    assert!(result.is_err(), "Should fail without task_id");
    let error = result.err().expect("should be Err");
    assert!(
        error.to_string().contains("task_id is required"),
        "Error should mention task_id required: {}",
        error
    );
}

#[tokio::test]
async fn test_documentation_get_nonexistent_task() {
    let ctx = TestContext::new().await;

    let project_path = "/tmp/test_doc_get_nonexistent".to_string();
    session_start(
        &ctx,
        project_path.clone(),
        Some("Doc Get Nonexistent Test".to_string()),
        None,
    )
    .await
    .expect("session_start failed");

    // Get non-existent task
    let result = documentation(
        &ctx,
        DocumentationRequest {
            action: DocumentationAction::Get,
            task_id: Some(99999),
            task_ids: None,
            reason: None,
            doc_type: None,
            priority: None,
            status: None,
            limit: None,
            offset: None,
        },
    )
    .await;

    assert!(result.is_err(), "Should fail for non-existent task");
    let error = result.err().expect("should be Err");
    assert!(
        error.to_string().contains("not found"),
        "Error should mention not found: {}",
        error
    );
}

#[tokio::test]
async fn test_documentation_complete_task() {
    use mira::tools::core::ToolContext;

    let ctx = TestContext::new().await;

    let project_path = "/tmp/test_doc_complete".to_string();
    session_start(
        &ctx,
        project_path.clone(),
        Some("Doc Complete Test".to_string()),
        None,
    )
    .await
    .expect("session_start failed");

    let project_id = ctx.project_id().await.expect("Should have project_id");

    // Create a pending doc task
    let task_id: i64 = ctx
        .pool()
        .run(move |conn| {
            conn.execute(
                "INSERT INTO documentation_tasks (
                    project_id, doc_type, doc_category, target_doc_path,
                    priority, status, reason
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    project_id,
                    "module",
                    "module",
                    "docs/modules/test.md",
                    "medium",
                    "pending",
                    "Module needs docs"
                ],
            )?;
            Ok::<i64, rusqlite::Error>(conn.last_insert_rowid())
        })
        .await
        .expect("Failed to create doc task");

    // Complete the task
    let result = documentation(
        &ctx,
        DocumentationRequest {
            action: DocumentationAction::Complete,
            task_id: Some(task_id),
            task_ids: None,
            reason: None,
            doc_type: None,
            priority: None,
            status: None,
            limit: None,
            offset: None,
        },
    )
    .await;

    assert!(
        result.is_ok(),
        "documentation complete failed: {:?}",
        result.err()
    );
    let output = result.unwrap();
    assert!(
        msg!(output).contains("marked complete"),
        "Output should confirm completion: {}",
        msg!(output)
    );

    // Verify status changed in database
    let status: String = ctx
        .pool()
        .run(move |conn| {
            conn.query_row(
                "SELECT status FROM documentation_tasks WHERE id = ?",
                [task_id],
                |row| row.get(0),
            )
        })
        .await
        .expect("Failed to query status");

    assert_eq!(status, "completed", "Status should be 'completed'");
}

#[tokio::test]
async fn test_documentation_complete_already_completed() {
    use mira::tools::core::ToolContext;

    let ctx = TestContext::new().await;

    let project_path = "/tmp/test_doc_complete_twice".to_string();
    session_start(
        &ctx,
        project_path.clone(),
        Some("Doc Complete Twice Test".to_string()),
        None,
    )
    .await
    .expect("session_start failed");

    let project_id = ctx.project_id().await.expect("Should have project_id");

    // Create an already-completed doc task
    let task_id: i64 = ctx
        .pool()
        .run(move |conn| {
            conn.execute(
                "INSERT INTO documentation_tasks (
                    project_id, doc_type, doc_category, target_doc_path,
                    priority, status, reason
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    project_id,
                    "module",
                    "module",
                    "docs/modules/already.md",
                    "medium",
                    "completed", // Already completed
                    "Already done"
                ],
            )?;
            Ok::<i64, rusqlite::Error>(conn.last_insert_rowid())
        })
        .await
        .expect("Failed to create doc task");

    // Try to complete again
    let result = documentation(
        &ctx,
        DocumentationRequest {
            action: DocumentationAction::Complete,
            task_id: Some(task_id),
            task_ids: None,
            reason: None,
            doc_type: None,
            priority: None,
            status: None,
            limit: None,
            offset: None,
        },
    )
    .await;

    assert!(result.is_err(), "Should fail for already-completed task");
    let error = result.err().expect("should be Err");
    assert!(
        error.to_string().contains("not pending"),
        "Error should mention not pending: {}",
        error
    );
}

#[tokio::test]
async fn test_documentation_skip_task() {
    use mira::tools::core::ToolContext;

    let ctx = TestContext::new().await;

    let project_path = "/tmp/test_doc_skip".to_string();
    session_start(
        &ctx,
        project_path.clone(),
        Some("Doc Skip Test".to_string()),
        None,
    )
    .await
    .expect("session_start failed");

    let project_id = ctx.project_id().await.expect("Should have project_id");

    // Create a pending doc task
    let task_id: i64 = ctx
        .pool()
        .run(move |conn| {
            conn.execute(
                "INSERT INTO documentation_tasks (
                    project_id, doc_type, doc_category, target_doc_path,
                    priority, status, reason
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    project_id,
                    "public_api",
                    "public_api",
                    "docs/api/skip.md",
                    "low",
                    "pending",
                    "API needs docs"
                ],
            )?;
            Ok::<i64, rusqlite::Error>(conn.last_insert_rowid())
        })
        .await
        .expect("Failed to create doc task");

    // Skip the task with a reason
    let result = documentation(
        &ctx,
        DocumentationRequest {
            action: DocumentationAction::Skip,
            task_id: Some(task_id),
            task_ids: None,
            reason: Some("Internal API, not needed".to_string()),
            doc_type: None,
            priority: None,
            status: None,
            limit: None,
            offset: None,
        },
    )
    .await;

    assert!(
        result.is_ok(),
        "documentation skip failed: {:?}",
        result.err()
    );
    let output = result.unwrap();
    assert!(
        msg!(output).contains("skipped"),
        "Output should confirm skip: {}",
        msg!(output)
    );
    assert!(
        msg!(output).contains("Internal API"),
        "Output should contain reason: {}",
        msg!(output)
    );

    // Verify status changed in database and original reason preserved
    let (status, reason, skip_reason): (String, String, Option<String>) = ctx
        .pool()
        .run(move |conn| {
            conn.query_row(
                "SELECT status, reason, skip_reason FROM documentation_tasks WHERE id = ?",
                [task_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
        })
        .await
        .expect("Failed to query status");

    assert_eq!(status, "skipped", "Status should be 'skipped'");
    assert!(
        reason.contains("API needs docs"),
        "Original reason should be preserved"
    );
    assert!(
        skip_reason.unwrap_or_default().contains("Internal API"),
        "Skip reason should be set"
    );
}

#[tokio::test]
async fn test_documentation_inventory_empty() {
    let ctx = TestContext::new().await;

    let project_path = "/tmp/test_doc_inventory".to_string();
    session_start(
        &ctx,
        project_path.clone(),
        Some("Doc Inventory Test".to_string()),
        None,
    )
    .await
    .expect("session_start failed");

    // Show inventory - should be empty
    let result = documentation(
        &ctx,
        DocumentationRequest {
            action: DocumentationAction::Inventory,
            task_id: None,
            task_ids: None,
            reason: None,
            doc_type: None,
            priority: None,
            status: None,
            limit: None,
            offset: None,
        },
    )
    .await;

    assert!(
        result.is_ok(),
        "documentation inventory failed: {:?}",
        result.err()
    );
    let output = result.unwrap();
    assert!(
        msg!(output).contains("No documentation inventory") || msg!(output).contains("Run scan"),
        "Output should indicate empty inventory: {}",
        msg!(output)
    );
}

#[tokio::test]
async fn test_documentation_scan() {
    let ctx = TestContext::new().await;

    let project_path = "/tmp/test_doc_scan".to_string();
    session_start(
        &ctx,
        project_path.clone(),
        Some("Doc Scan Test".to_string()),
        None,
    )
    .await
    .expect("session_start failed");

    // Trigger scan
    let result = documentation(
        &ctx,
        DocumentationRequest {
            action: DocumentationAction::Scan,
            task_id: None,
            task_ids: None,
            reason: None,
            doc_type: None,
            priority: None,
            status: None,
            limit: None,
            offset: None,
        },
    )
    .await;

    assert!(
        result.is_ok(),
        "documentation scan failed: {:?}",
        result.err()
    );
    let output = result.unwrap();
    assert!(
        msg!(output).contains("scan triggered"),
        "Output should confirm scan triggered: {}",
        msg!(output)
    );
}

#[tokio::test]
async fn test_documentation_project_scoping() {
    use mira::tools::core::ToolContext;

    let ctx = TestContext::new().await;

    // Create first project and add a doc task
    let project1_path = "/tmp/test_doc_scope_1".to_string();
    session_start(
        &ctx,
        project1_path.clone(),
        Some("Project 1".to_string()),
        None,
    )
    .await
    .expect("session_start failed");

    let project1_id = ctx.project_id().await.expect("Should have project_id");

    let task1_id: i64 = ctx
        .pool()
        .run(move |conn| {
            conn.execute(
                "INSERT INTO documentation_tasks (
                    project_id, doc_type, doc_category, target_doc_path,
                    priority, status, reason
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    project1_id,
                    "module",
                    "module",
                    "docs/project1.md",
                    "high",
                    "pending",
                    "Project 1 docs"
                ],
            )?;
            Ok::<i64, rusqlite::Error>(conn.last_insert_rowid())
        })
        .await
        .expect("Failed to create doc task");

    // Switch to second project
    let project2_path = "/tmp/test_doc_scope_2".to_string();
    session_start(
        &ctx,
        project2_path.clone(),
        Some("Project 2".to_string()),
        None,
    )
    .await
    .expect("session_start for project 2 failed");

    let project2_id = ctx.project_id().await.expect("Should have project_id");
    assert_ne!(project1_id, project2_id, "Should be different projects");

    // Try to get task from project 1 while in project 2 - should fail
    let result = documentation(
        &ctx,
        DocumentationRequest {
            action: DocumentationAction::Get,
            task_id: Some(task1_id),
            task_ids: None,
            reason: None,
            doc_type: None,
            priority: None,
            status: None,
            limit: None,
            offset: None,
        },
    )
    .await;

    assert!(
        result.is_err(),
        "Should not access task from different project"
    );
    let error = result.err().expect("should be Err");
    assert!(
        error.to_string().contains("different project"),
        "Error should mention different project: {}",
        error
    );

    // Try to complete task from project 1 while in project 2 - should fail
    let result = documentation(
        &ctx,
        DocumentationRequest {
            action: DocumentationAction::Complete,
            task_id: Some(task1_id),
            task_ids: None,
            reason: None,
            doc_type: None,
            priority: None,
            status: None,
            limit: None,
            offset: None,
        },
    )
    .await;

    assert!(
        result.is_err(),
        "Should not complete task from different project"
    );

    // Try to skip task from project 1 while in project 2 - should fail
    let result = documentation(
        &ctx,
        DocumentationRequest {
            action: DocumentationAction::Skip,
            task_id: Some(task1_id),
            task_ids: None,
            reason: Some("test".to_string()),
            doc_type: None,
            priority: None,
            status: None,
            limit: None,
            offset: None,
        },
    )
    .await;

    assert!(
        result.is_err(),
        "Should not skip task from different project"
    );

    // List should only show tasks for current project (project 2 has none)
    let result = documentation(
        &ctx,
        DocumentationRequest {
            action: DocumentationAction::List,
            task_id: None,
            task_ids: None,
            reason: None,
            doc_type: None,
            priority: None,
            status: None,
            limit: None,
            offset: None,
        },
    )
    .await;

    assert!(
        result.is_ok(),
        "documentation list failed: {:?}",
        result.err()
    );
    let output = result.unwrap();
    assert!(
        msg!(output).contains("No documentation tasks found"),
        "Should not see project 1 tasks: {}",
        msg!(output)
    );
}

#[tokio::test]
async fn test_documentation_list_filter_by_status() {
    use mira::tools::core::ToolContext;

    let ctx = TestContext::new().await;

    let project_path = "/tmp/test_doc_filter_status".to_string();
    session_start(
        &ctx,
        project_path.clone(),
        Some("Doc Filter Test".to_string()),
        None,
    )
    .await
    .expect("session_start failed");

    let project_id = ctx.project_id().await.expect("Should have project_id");

    // Create tasks with different statuses
    ctx.pool()
        .run(move |conn| {
            conn.execute(
                "INSERT INTO documentation_tasks (
                    project_id, doc_type, doc_category, target_doc_path,
                    priority, status, reason
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    project_id,
                    "module",
                    "module",
                    "docs/pending.md",
                    "high",
                    "pending",
                    "Pending task"
                ],
            )?;
            conn.execute(
                "INSERT INTO documentation_tasks (
                    project_id, doc_type, doc_category, target_doc_path,
                    priority, status, reason
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    project_id,
                    "module",
                    "module",
                    "docs/applied.md",
                    "medium",
                    "completed",
                    "Completed task"
                ],
            )?;
            Ok::<(), rusqlite::Error>(())
        })
        .await
        .expect("Failed to create doc tasks");

    // List only pending tasks
    let result = documentation(
        &ctx,
        DocumentationRequest {
            action: DocumentationAction::List,
            task_id: None,
            task_ids: None,
            reason: None,
            doc_type: None,
            priority: None,
            status: Some("pending".to_string()),
            limit: None,
            offset: None,
        },
    )
    .await;

    assert!(
        result.is_ok(),
        "documentation list failed: {:?}",
        result.err()
    );
    let output = result.unwrap();
    assert!(
        msg!(output).contains("pending.md"),
        "Should show pending task: {}",
        msg!(output)
    );
    assert!(
        !msg!(output).contains("applied.md"),
        "Should not show applied task: {}",
        msg!(output)
    );
}

// ============================================================================
// Tasks fallback tool tests
// ============================================================================

use mira::mcp::MiraServer;
use rmcp::task_manager::{OperationDescriptor, OperationMessage, ToolCallTaskResult};

/// Helper to create a MiraServer with in-memory DBs for task tests
async fn make_task_server() -> MiraServer {
    let pool = mira::db::pool::MainPool::new(Arc::new(
        mira::db::pool::DatabasePool::open_in_memory()
            .await
            .expect("pool"),
    ));
    let code_pool = mira::db::pool::CodePool::new(Arc::new(
        mira::db::pool::DatabasePool::open_code_db_in_memory()
            .await
            .expect("code pool"),
    ));
    MiraServer::new(pool, code_pool, None)
}

#[tokio::test]
async fn test_tasks_list_empty() {
    let server = make_task_server().await;
    let output = mira::tools::tasks::handle_tasks(&server, TaskAction::List, None)
        .await
        .expect("tasks list should succeed");
    assert!(
        msg!(output).contains("No tasks"),
        "Expected 'No tasks', got: {}",
        msg!(output)
    );
}

#[tokio::test]
async fn test_tasks_get_not_found() {
    let server = make_task_server().await;
    let result = mira::tools::tasks::handle_tasks(
        &server,
        TaskAction::Get,
        Some("nonexistent-id".to_string()),
    )
    .await;
    let err = result.err().expect("expected error");
    assert!(
        err.to_string().contains("not found"),
        "Expected 'not found' error, got: {}",
        err
    );
}

#[tokio::test]
async fn test_tasks_cancel_not_found() {
    let server = make_task_server().await;
    let result = mira::tools::tasks::handle_tasks(
        &server,
        TaskAction::Cancel,
        Some("nonexistent-id".to_string()),
    )
    .await;
    let err = result.err().expect("expected error");
    assert!(
        err.to_string().contains("not found"),
        "Expected 'not found' error, got: {}",
        err
    );
}

#[tokio::test]
async fn test_tasks_get_missing_task_id() {
    let server = make_task_server().await;
    let result = mira::tools::tasks::handle_tasks(&server, TaskAction::Get, None).await;
    let err = result.err().expect("expected error");
    assert!(
        err.to_string().contains("task_id is required"),
        "Expected 'task_id is required' error, got: {}",
        err
    );
}

#[tokio::test]
async fn test_tasks_lifecycle() {
    let server = make_task_server().await;

    // Manually submit a short operation to the processor
    let task_id = "test-task-123".to_string();
    let tid = task_id.clone();
    let future: rmcp::task_manager::OperationFuture = Box::pin(async move {
        // Simulate a short operation
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let result = rmcp::model::CallToolResult {
            content: vec![rmcp::model::Content::text("Task completed successfully")],
            structured_content: Some(serde_json::json!({"status": "done"})),
            is_error: Some(false),
            meta: None,
        };
        let transport = ToolCallTaskResult::new(tid, Ok(result));
        Ok(Box::new(transport) as Box<dyn rmcp::task_manager::OperationResultTransport>)
    });

    let descriptor = OperationDescriptor::new(task_id.clone(), "test_tool").with_ttl(60);
    let message = OperationMessage::new(descriptor, future);

    {
        let mut proc = server.processor.lock().await;
        proc.submit_operation(message)
            .expect("submit should succeed");
    }

    // List — should show one working task
    let output = mira::tools::tasks::handle_tasks(&server, TaskAction::List, None)
        .await
        .expect("tasks list should succeed");
    assert!(
        msg!(output).contains("1 task(s)"),
        "Expected 1 task, got: {}",
        msg!(output)
    );

    // Wait for the operation to complete
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // Get — should return completed result
    let output = mira::tools::tasks::handle_tasks(&server, TaskAction::Get, Some(task_id.clone()))
        .await
        .expect("tasks get should succeed");
    assert!(
        msg!(output).contains("completed"),
        "Expected 'completed' status, got: {}",
        msg!(output)
    );
    assert!(
        msg!(output).contains("Task completed successfully"),
        "Expected result text, got: {}",
        msg!(output)
    );
}

#[tokio::test]
async fn test_tasks_cancel_running() {
    let server = make_task_server().await;

    // Submit a slow operation
    let task_id = "slow-task-456".to_string();
    let tid = task_id.clone();
    let future: rmcp::task_manager::OperationFuture = Box::pin(async move {
        // Very long sleep — will be cancelled
        tokio::time::sleep(std::time::Duration::from_secs(300)).await;
        let result = rmcp::model::CallToolResult {
            content: vec![rmcp::model::Content::text("should not reach here")],
            structured_content: None,
            is_error: Some(false),
            meta: None,
        };
        let transport = ToolCallTaskResult::new(tid, Ok(result));
        Ok(Box::new(transport) as Box<dyn rmcp::task_manager::OperationResultTransport>)
    });

    let descriptor = OperationDescriptor::new(task_id.clone(), "slow_tool").with_ttl(300);
    let message = OperationMessage::new(descriptor, future);

    {
        let mut proc = server.processor.lock().await;
        proc.submit_operation(message)
            .expect("submit should succeed");
    }

    // Cancel
    let output =
        mira::tools::tasks::handle_tasks(&server, TaskAction::Cancel, Some(task_id.clone()))
            .await
            .expect("cancel should succeed");
    assert!(
        msg!(output).contains("cancelled"),
        "Expected 'cancelled' message, got: {}",
        msg!(output)
    );

    // Get after cancel — should show cancelled status
    let output = mira::tools::tasks::handle_tasks(&server, TaskAction::Get, Some(task_id.clone()))
        .await
        .expect("get after cancel should succeed");
    assert!(
        msg!(output).contains("cancelled"),
        "Expected 'cancelled' status after cancel, got: {}",
        msg!(output)
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Goal Tests (Phase 1)
// ═══════════════════════════════════════════════════════════════════════════════

/// Helper to create a goal and return its ID
async fn create_test_goal(ctx: &TestContext, title: &str) -> i64 {
    let output = goal(
        ctx,
        GoalRequest {
            action: GoalAction::Create,
            goal_id: None,
            title: Some(title.to_string()),
            description: Some(format!("Description for {}", title)),
            status: Some("planning".to_string()),
            priority: Some("high".to_string()),
            progress_percent: None,
            include_finished: None,
            milestone_id: None,
            milestone_title: None,
            weight: None,
            limit: None,
            goals: None,
        },
    )
    .await
    .expect("goal create failed");

    match &output.0.data {
        Some(GoalData::Created(data)) => data.goal_id,
        other => panic!("Expected Created data, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_goal_get() {
    let ctx = TestContext::new().await;
    session_start(
        &ctx,
        "/tmp/test_goal_get".into(),
        Some("Goal Get".into()),
        None,
    )
    .await
    .expect("session_start failed");

    let goal_id = create_test_goal(&ctx, "Get test goal").await;

    let output = goal(
        &ctx,
        GoalRequest {
            action: GoalAction::Get,
            goal_id: Some(goal_id),
            title: None,
            description: None,
            status: None,
            priority: None,
            progress_percent: None,
            include_finished: None,
            milestone_id: None,
            milestone_title: None,
            weight: None,
            limit: None,
            goals: None,
        },
    )
    .await
    .expect("goal get failed");

    assert!(
        msg!(output).contains("Get test goal"),
        "Output: {}",
        msg!(output)
    );
    assert!(
        msg!(output).contains("planning"),
        "Output: {}",
        msg!(output)
    );
    assert!(
        msg!(output).contains("Description for"),
        "Output: {}",
        msg!(output)
    );

    match &output.0.data {
        Some(GoalData::Get(data)) => {
            assert_eq!(data.id, goal_id);
            assert_eq!(data.title, "Get test goal");
            assert_eq!(data.status, "planning");
            assert_eq!(data.priority, "high");
        }
        other => panic!("Expected Get data, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_goal_update() {
    let ctx = TestContext::new().await;
    session_start(
        &ctx,
        "/tmp/test_goal_update".into(),
        Some("Goal Update".into()),
        None,
    )
    .await
    .expect("session_start failed");

    let goal_id = create_test_goal(&ctx, "Update test goal").await;

    // Update title and status
    let output = goal(
        &ctx,
        GoalRequest {
            action: GoalAction::Update,
            goal_id: Some(goal_id),
            title: Some("Updated title".to_string()),
            description: None,
            status: Some("in_progress".to_string()),
            priority: None,
            progress_percent: None,
            include_finished: None,
            milestone_id: None,
            milestone_title: None,
            weight: None,
            limit: None,
            goals: None,
        },
    )
    .await
    .expect("goal update failed");

    assert!(msg!(output).contains("Updated"), "Output: {}", msg!(output));

    // Verify via get
    let get_output = goal(
        &ctx,
        GoalRequest {
            action: GoalAction::Get,
            goal_id: Some(goal_id),
            title: None,
            description: None,
            status: None,
            priority: None,
            progress_percent: None,
            include_finished: None,
            milestone_id: None,
            milestone_title: None,
            weight: None,
            limit: None,
            goals: None,
        },
    )
    .await
    .expect("goal get failed");

    match &get_output.0.data {
        Some(GoalData::Get(data)) => {
            assert_eq!(data.title, "Updated title");
            assert_eq!(data.status, "in_progress");
        }
        other => panic!("Expected Get data, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_goal_delete() {
    let ctx = TestContext::new().await;
    session_start(
        &ctx,
        "/tmp/test_goal_delete".into(),
        Some("Goal Delete".into()),
        None,
    )
    .await
    .expect("session_start failed");

    let goal_id = create_test_goal(&ctx, "Delete test goal").await;

    // Delete
    let output = goal(
        &ctx,
        GoalRequest {
            action: GoalAction::Delete,
            goal_id: Some(goal_id),
            title: None,
            description: None,
            status: None,
            priority: None,
            progress_percent: None,
            include_finished: None,
            milestone_id: None,
            milestone_title: None,
            weight: None,
            limit: None,
            goals: None,
        },
    )
    .await
    .expect("goal delete failed");

    assert!(
        msg!(output).contains("Deleted") || msg!(output).contains("deleted"),
        "Output: {}",
        msg!(output)
    );

    // Verify list is empty
    let list_output = goal(
        &ctx,
        GoalRequest {
            action: GoalAction::List,
            goal_id: None,
            title: None,
            description: None,
            status: None,
            priority: None,
            progress_percent: None,
            include_finished: Some(false),
            milestone_id: None,
            milestone_title: None,
            weight: None,
            limit: Some(10),
            goals: None,
        },
    )
    .await
    .expect("goal list failed");

    assert!(
        !msg!(list_output).contains("Delete test goal"),
        "Deleted goal should not appear: {}",
        msg!(list_output)
    );
}

#[tokio::test]
async fn test_goal_milestone_lifecycle() {
    let ctx = TestContext::new().await;
    session_start(
        &ctx,
        "/tmp/test_goal_milestone".into(),
        Some("Goal Milestone".into()),
        None,
    )
    .await
    .expect("session_start failed");

    let goal_id = create_test_goal(&ctx, "Milestone test goal").await;

    // Add milestone
    let add_output = goal(
        &ctx,
        GoalRequest {
            action: GoalAction::AddMilestone,
            goal_id: Some(goal_id),
            title: None,
            description: None,
            status: None,
            priority: None,
            progress_percent: None,
            include_finished: None,
            milestone_id: None,
            milestone_title: Some("First milestone".to_string()),
            weight: Some(1),
            limit: None,
            goals: None,
        },
    )
    .await
    .expect("add milestone failed");

    assert!(
        msg!(add_output).contains("milestone"),
        "Output: {}",
        msg!(add_output)
    );

    // Extract milestone ID
    let milestone_id = match &add_output.0.data {
        Some(GoalData::MilestoneProgress(data)) => data.milestone_id,
        other => panic!("Expected MilestoneProgress data, got: {:?}", other),
    };

    // Complete milestone
    let complete_output = goal(
        &ctx,
        GoalRequest {
            action: GoalAction::CompleteMilestone,
            goal_id: None,
            title: None,
            description: None,
            status: None,
            priority: None,
            progress_percent: None,
            include_finished: None,
            milestone_id: Some(milestone_id),
            milestone_title: None,
            weight: None,
            limit: None,
            goals: None,
        },
    )
    .await
    .expect("complete milestone failed");

    assert!(
        msg!(complete_output).contains("Completed") || msg!(complete_output).contains("completed"),
        "Output: {}",
        msg!(complete_output)
    );

    // Verify progress auto-calculated to 100%
    match &complete_output.0.data {
        Some(GoalData::MilestoneProgress(data)) => {
            assert_eq!(data.progress_percent, Some(100));
        }
        other => panic!("Expected MilestoneProgress data, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_goal_list_with_milestones() {
    let ctx = TestContext::new().await;
    session_start(
        &ctx,
        "/tmp/test_goal_list_ms".into(),
        Some("Goal List Milestones".into()),
        None,
    )
    .await
    .expect("session_start failed");

    let goal_id = create_test_goal(&ctx, "List milestones goal").await;

    // Add 2 milestones
    for title in &["Milestone A", "Milestone B"] {
        goal(
            &ctx,
            GoalRequest {
                action: GoalAction::AddMilestone,
                goal_id: Some(goal_id),
                title: None,
                description: None,
                status: None,
                priority: None,
                progress_percent: None,
                include_finished: None,
                milestone_id: None,
                milestone_title: Some(title.to_string()),
                weight: Some(1),
                limit: None,
                goals: None,
            },
        )
        .await
        .expect("add milestone failed");
    }

    // List goals - milestones should appear inline
    let list_output = goal(
        &ctx,
        GoalRequest {
            action: GoalAction::List,
            goal_id: None,
            title: None,
            description: None,
            status: None,
            priority: None,
            progress_percent: None,
            include_finished: None,
            milestone_id: None,
            milestone_title: None,
            weight: None,
            limit: Some(10),
            goals: None,
        },
    )
    .await
    .expect("goal list failed");

    assert!(
        msg!(list_output).contains("List milestones goal"),
        "Output: {}",
        msg!(list_output)
    );
    assert!(
        msg!(list_output).contains("Milestone A"),
        "Output should contain milestone A: {}",
        msg!(list_output)
    );
    assert!(
        msg!(list_output).contains("Milestone B"),
        "Output should contain milestone B: {}",
        msg!(list_output)
    );

    // Verify structured data has milestones
    match &list_output.0.data {
        Some(GoalData::List(data)) => {
            assert_eq!(data.goals.len(), 1);
            assert_eq!(data.goals[0].milestones.len(), 2);
        }
        other => panic!("Expected List data, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_goal_bulk_create() {
    let ctx = TestContext::new().await;
    session_start(
        &ctx,
        "/tmp/test_goal_bulk".into(),
        Some("Goal Bulk".into()),
        None,
    )
    .await
    .expect("session_start failed");

    let goals_json = serde_json::json!([
        {"title": "Bulk goal 1", "priority": "high"},
        {"title": "Bulk goal 2", "priority": "medium"},
        {"title": "Bulk goal 3"}
    ])
    .to_string();

    let output = goal(
        &ctx,
        GoalRequest {
            action: GoalAction::BulkCreate,
            goal_id: None,
            title: None,
            description: None,
            status: None,
            priority: None,
            progress_percent: None,
            include_finished: None,
            milestone_id: None,
            milestone_title: None,
            weight: None,
            limit: None,
            goals: Some(goals_json),
        },
    )
    .await
    .expect("bulk create failed");

    assert!(
        msg!(output).contains("Created 3") || msg!(output).contains("3 goals"),
        "Output: {}",
        msg!(output)
    );

    match &output.0.data {
        Some(GoalData::BulkCreated(data)) => {
            assert_eq!(data.goals.len(), 3);
            assert_eq!(data.goals[0].title, "Bulk goal 1");
            assert_eq!(data.goals[1].title, "Bulk goal 2");
            assert_eq!(data.goals[2].title, "Bulk goal 3");
        }
        other => panic!("Expected BulkCreated data, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_goal_progress_update() {
    let ctx = TestContext::new().await;
    session_start(
        &ctx,
        "/tmp/test_goal_progress".into(),
        Some("Goal Progress".into()),
        None,
    )
    .await
    .expect("session_start failed");

    let goal_id = create_test_goal(&ctx, "Progress test goal").await;

    // Set progress to 50%
    let output = goal(
        &ctx,
        GoalRequest {
            action: GoalAction::Update,
            goal_id: Some(goal_id),
            title: None,
            description: None,
            status: None,
            priority: None,
            progress_percent: Some(50),
            include_finished: None,
            milestone_id: None,
            milestone_title: None,
            weight: None,
            limit: None,
            goals: None,
        },
    )
    .await
    .expect("goal progress failed");

    assert!(
        msg!(output).contains("50")
            || msg!(output).contains("Updated")
            || msg!(output).contains("progress"),
        "Output: {}",
        msg!(output)
    );

    // Verify via get
    let get_output = goal(
        &ctx,
        GoalRequest {
            action: GoalAction::Get,
            goal_id: Some(goal_id),
            title: None,
            description: None,
            status: None,
            priority: None,
            progress_percent: None,
            include_finished: None,
            milestone_id: None,
            milestone_title: None,
            weight: None,
            limit: None,
            goals: None,
        },
    )
    .await
    .expect("goal get failed");

    match &get_output.0.data {
        Some(GoalData::Get(data)) => {
            assert_eq!(data.progress_percent, 50);
        }
        other => panic!("Expected Get data, got: {:?}", other),
    }
}

// =========================================================================
// Dismiss Insight Tests
// =========================================================================

/// Helper: insert a behavior_patterns row and return its id
async fn insert_behavior_pattern(
    ctx: &TestContext,
    project_id: i64,
    pattern_type: &str,
    pattern_key: &str,
) -> i64 {
    let pt = pattern_type.to_string();
    let pk = pattern_key.to_string();
    ctx.pool()
        .run(move |conn| {
            conn.execute(
                "INSERT INTO behavior_patterns (project_id, pattern_type, pattern_key, pattern_data, confidence, last_triggered_at)
                 VALUES (?1, ?2, ?3, '{\"description\":\"test insight\"}', 0.8, datetime('now'))",
                rusqlite::params![project_id, pt, pk],
            )
            .map_err(|e| e.to_string())?;
            Ok::<i64, String>(conn.last_insert_rowid())
        })
        .await
        .expect("Failed to insert behavior_pattern")
}

/// Helper: check if a row has dismissed = 1
async fn is_dismissed(ctx: &TestContext, row_id: i64) -> bool {
    ctx.pool()
        .run(move |conn| {
            let dismissed: i64 = conn
                .query_row(
                    "SELECT COALESCE(dismissed, 0) FROM behavior_patterns WHERE id = ?1",
                    rusqlite::params![row_id],
                    |row| row.get(0),
                )
                .map_err(|e| e.to_string())?;
            Ok::<bool, String>(dismissed == 1)
        })
        .await
        .expect("Failed to query dismissed status")
}

#[tokio::test]
async fn test_dismiss_insight_success() {
    use mira::mcp::requests::{SessionAction, SessionRequest};
    let ctx = TestContext::new().await;

    // Set up a project
    session_start(
        &ctx,
        "/tmp/test_dismiss".into(),
        Some("Dismiss Test".into()),
        None,
    )
    .await
    .expect("session_start failed");
    let project = ctx.get_project().await.expect("project should be set");

    // Insert an insight for this project
    let row_id =
        insert_behavior_pattern(&ctx, project.id, "insight_fragile_code", "test_dismiss_1").await;

    // Dismiss it
    let req = SessionRequest {
        action: SessionAction::DismissInsight,
        session_id: None,

        limit: None,
        group_by: None,
        since_days: None,
        insight_source: Some("pondering".into()),
        min_confidence: None,
        insight_id: Some(row_id),
        dry_run: None,
        category: None,
    };
    let result = handle_session(&ctx, req).await;
    assert!(result.is_ok(), "dismiss_insight failed: {:?}", result.err());
    let output = result.unwrap();
    assert!(
        msg!(output).contains("dismissed"),
        "Expected 'dismissed' in output: {}",
        msg!(output)
    );
    assert!(is_dismissed(&ctx, row_id).await, "Row should be dismissed");
}

#[tokio::test]
async fn test_dismiss_insight_cross_project_blocked() {
    use mira::mcp::requests::{SessionAction, SessionRequest};
    let ctx = TestContext::new().await;

    // Set up project A and insert an insight
    session_start(
        &ctx,
        "/tmp/test_dismiss_a".into(),
        Some("Project A".into()),
        None,
    )
    .await
    .expect("session_start failed");
    let project_a = ctx.get_project().await.expect("project A should be set");
    let row_id = insert_behavior_pattern(
        &ctx,
        project_a.id,
        "insight_fragile_code",
        "cross_project_1",
    )
    .await;

    // Switch to project B
    session_start(
        &ctx,
        "/tmp/test_dismiss_b".into(),
        Some("Project B".into()),
        None,
    )
    .await
    .expect("session_start failed");

    // Try to dismiss project A's insight from project B's context
    let req = SessionRequest {
        action: SessionAction::DismissInsight,
        session_id: None,

        limit: None,
        group_by: None,
        since_days: None,
        insight_source: Some("pondering".into()),
        min_confidence: None,
        insight_id: Some(row_id),
        dry_run: None,
        category: None,
    };
    let result = handle_session(&ctx, req).await;
    assert!(
        result.is_ok(),
        "dismiss_insight should not error: {:?}",
        result.err()
    );
    let output = result.unwrap();
    assert!(
        msg!(output).contains("not found"),
        "Expected 'not found' for cross-project dismiss: {}",
        msg!(output)
    );
    assert!(
        !is_dismissed(&ctx, row_id).await,
        "Row should NOT be dismissed"
    );
}

#[tokio::test]
async fn test_dismiss_insight_non_insight_pattern_blocked() {
    use mira::mcp::requests::{SessionAction, SessionRequest};
    let ctx = TestContext::new().await;

    // Set up project
    session_start(
        &ctx,
        "/tmp/test_dismiss_type".into(),
        Some("Type Test".into()),
        None,
    )
    .await
    .expect("session_start failed");
    let project = ctx.get_project().await.expect("project should be set");

    // Insert a non-insight behavior pattern (e.g., file_sequence)
    let row_id = insert_behavior_pattern(&ctx, project.id, "file_sequence", "not_an_insight").await;

    // Try to dismiss it — should be blocked by pattern_type filter
    let req = SessionRequest {
        action: SessionAction::DismissInsight,
        session_id: None,

        limit: None,
        group_by: None,
        since_days: None,
        insight_source: Some("pondering".into()),
        min_confidence: None,
        insight_id: Some(row_id),
        dry_run: None,
        category: None,
    };
    let result = handle_session(&ctx, req).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    assert!(
        msg!(output).contains("not found"),
        "Non-insight rows should not be dismissable: {}",
        msg!(output)
    );
    assert!(
        !is_dismissed(&ctx, row_id).await,
        "Non-insight row should NOT be dismissed"
    );
}

#[tokio::test]
async fn test_dismiss_insight_requires_project() {
    use mira::mcp::requests::{SessionAction, SessionRequest};
    let ctx = TestContext::new().await;

    // No project set — should fail with project error
    let req = SessionRequest {
        action: SessionAction::DismissInsight,
        session_id: None,

        limit: None,
        group_by: None,
        since_days: None,
        insight_source: Some("pondering".into()),
        min_confidence: None,
        insight_id: Some(999),
        dry_run: None,
        category: None,
    };
    let result = handle_session(&ctx, req).await;
    assert!(result.is_err(), "Should fail without active project");
    let err = result.err().unwrap();
    assert!(
        err.to_string().contains("project"),
        "Error should mention project, got: {}",
        err
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Hook CLI Smoke Tests
// ═══════════════════════════════════════════════════════════════════════════════

/// Smoke test: `mira hook session-start` should not panic.
/// This catches regressions like the Handle::block_on panic inside #[tokio::main].
#[test]
fn hook_session_start_no_panic() {
    let binary = env!("CARGO_BIN_EXE_mira");
    let output = std::process::Command::new(binary)
        .args(["hook", "session-start"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(ref mut stdin) = child.stdin {
                let _ = stdin
                    .write_all(br#"{"session_id":"test-smoke","cwd":"/tmp","source":"startup"}"#);
            }
            child.wait_with_output()
        })
        .expect("Failed to run mira binary");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("panicked"),
        "Hook should not panic. stderr: {}",
        stderr
    );
    assert!(
        output.status.success(),
        "Hook should exit 0. stderr: {}",
        stderr
    );
}

/// Smoke test: `mira hook pre-tool` should not panic on supported tools.
#[test]
fn hook_pre_tool_no_panic() {
    let binary = env!("CARGO_BIN_EXE_mira");
    let output = std::process::Command::new(binary)
        .args(["hook", "pre-tool"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(ref mut stdin) = child.stdin {
                let _ = stdin.write_all(
                    br#"{"tool_name":"Grep","tool_input":{"pattern":"test","path":"/tmp"}}"#,
                );
            }
            child.wait_with_output()
        })
        .expect("Failed to run mira binary");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("panicked"),
        "Hook should not panic. stderr: {}",
        stderr
    );
    assert!(
        output.status.success(),
        "Hook should exit 0. stderr: {}",
        stderr
    );
}

/// Smoke test: `mira hook user-prompt` should not panic.
#[test]
fn hook_user_prompt_no_panic() {
    let binary = env!("CARGO_BIN_EXE_mira");
    let output = std::process::Command::new(binary)
        .args(["hook", "user-prompt"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(ref mut stdin) = child.stdin {
                let _ = stdin.write_all(br#"{"session_id":"test-smoke","prompt":"hello world"}"#);
            }
            child.wait_with_output()
        })
        .expect("Failed to run mira binary");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("panicked"),
        "Hook should not panic. stderr: {}",
        stderr
    );
    assert!(
        output.status.success(),
        "Hook should exit 0. stderr: {}",
        stderr
    );
}

// =========================================================================
// Insights Empty-State Message Tests
// =========================================================================

#[tokio::test]
async fn test_insights_empty_no_data_shows_setup_instructions() {
    use mira::mcp::requests::{SessionAction, SessionRequest};
    let ctx = TestContext::new().await;

    session_start(
        &ctx,
        "/tmp/test_insights_empty".into(),
        Some("Insights Empty".into()),
        None,
    )
    .await
    .expect("session_start failed");

    // No insights, no health snapshots → should show setup instructions
    let req = SessionRequest {
        action: SessionAction::Insights,
        session_id: None,

        limit: None,
        group_by: None,
        since_days: None,
        insight_source: None,
        min_confidence: None,
        insight_id: None,
        dry_run: None,
        category: None,
    };
    let result = handle_session(&ctx, req).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    assert!(
        msg!(output).contains("index(action="),
        "Empty state with no data should show setup instructions, got: {}",
        msg!(output)
    );
}

// Health snapshot "healthy" test removed — health_snapshots table dropped in v50.

#[tokio::test]
async fn test_insights_empty_with_filters_shows_filter_message() {
    use mira::mcp::requests::{SessionAction, SessionRequest};
    let ctx = TestContext::new().await;

    session_start(
        &ctx,
        "/tmp/test_insights_filter".into(),
        Some("Insights Filter".into()),
        None,
    )
    .await
    .expect("session_start failed");

    // With insight_source filter and no insights → should mention filters
    let req = SessionRequest {
        action: SessionAction::Insights,
        session_id: None,

        limit: None,
        group_by: None,
        since_days: None,
        insight_source: Some("pondering".into()),
        min_confidence: None,
        insight_id: None,
        dry_run: None,
        category: None,
    };
    let result = handle_session(&ctx, req).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    assert!(
        msg!(output).contains("filters"),
        "Filtered empty state should mention filters, got: {}",
        msg!(output)
    );
    assert!(
        !msg!(output).contains("healthy"),
        "Filtered empty state should NOT say healthy, got: {}",
        msg!(output)
    );

    // With since_days filter → same behavior
    let req2 = SessionRequest {
        action: SessionAction::Insights,
        session_id: None,

        limit: None,
        group_by: None,
        since_days: Some(1),
        insight_source: None,
        min_confidence: None,
        insight_id: None,
        dry_run: None,
        category: None,
    };
    let result2 = handle_session(&ctx, req2).await;
    assert!(result2.is_ok());
    let output2 = result2.unwrap();
    assert!(
        msg!(output2).contains("filters"),
        "since_days filter should show filter message, got: {}",
        msg!(output2)
    );

    // With limit filter → same behavior
    let req3 = SessionRequest {
        action: SessionAction::Insights,
        session_id: None,

        limit: Some(0),
        group_by: None,
        since_days: None,
        insight_source: None,
        min_confidence: None,
        insight_id: None,
        dry_run: None,
        category: None,
    };
    let result3 = handle_session(&ctx, req3).await;
    assert!(result3.is_ok());
    let output3 = result3.unwrap();
    assert!(
        msg!(output3).contains("filters"),
        "limit=0 filter should show filter message, got: {}",
        msg!(output3)
    );
}

/// Smoke test: `mira hook stop` should not panic.
#[test]
fn hook_stop_no_panic() {
    let binary = env!("CARGO_BIN_EXE_mira");
    let output = std::process::Command::new(binary)
        .args(["hook", "stop"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(ref mut stdin) = child.stdin {
                let _ = stdin.write_all(br#"{"session_id":"test-smoke"}"#);
            }
            child.wait_with_output()
        })
        .expect("Failed to run mira binary");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("panicked"),
        "Hook should not panic. stderr: {}",
        stderr
    );
    assert!(
        output.status.success(),
        "Hook should exit 0. stderr: {}",
        stderr
    );
}

// ============================================================================
// Launch tool tests
// ============================================================================

#[tokio::test]
async fn test_launch_team_not_found() {
    let ctx = TestContext::new().await;

    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().to_str().unwrap().to_string();
    session_start(&ctx, path, None, None).await.unwrap();

    let result = handle_launch(&ctx, "nonexistent-team".into(), None, None, None).await;
    assert!(result.is_err(), "Should fail when team file doesn't exist");
    let err = format!("{}", result.err().unwrap());
    assert!(
        err.contains("not found") || err.contains("No such file"),
        "Error should mention file not found: {}",
        err
    );
}

#[tokio::test]
async fn test_launch_requires_project() {
    let ctx = TestContext::new().await;

    let result = handle_launch(&ctx, "expert-review-team".into(), None, None, None).await;
    assert!(result.is_err(), "Should fail when no project is active");
}

#[tokio::test]
async fn test_launch_parses_agent_file() {
    let ctx = TestContext::new().await;

    let tmp = tempfile::tempdir().unwrap();
    let agents_dir = tmp.path().join(".claude").join("agents");
    std::fs::create_dir_all(&agents_dir).unwrap();
    std::fs::write(
        agents_dir.join("test-team.md"),
        r#"---
name: test-team
description: A test team for integration testing
---

### Alice -- Code Reviewer

**Personality:** Thorough and detail-oriented

**Focus:** Code quality and correctness

**Tools:** Read-only (Glob, Grep, Read)

### Bob -- Security Analyst

**Personality:** Cautious and methodical

**Focus:** Security vulnerabilities and data handling

**Tools:** Full tools
"#,
    )
    .unwrap();

    let path = tmp.path().to_str().unwrap().to_string();
    session_start(&ctx, path, None, None).await.unwrap();

    let result = handle_launch(&ctx, "test-team".into(), None, None, None).await;
    assert!(result.is_ok(), "Launch should succeed: {}", result.as_ref().err().map(|e| e.to_string()).unwrap_or_default());
    let output = result.unwrap();
    let data = output.0.data.as_ref().unwrap();

    assert_eq!(data.team_name, "test-team");
    assert_eq!(data.team_description, "A test team for integration testing");
    assert_eq!(data.agents.len(), 2);

    assert_eq!(data.agents[0].name, "alice");
    assert_eq!(data.agents[0].role, "Code Reviewer");
    assert!(data.agents[0].read_only);
    assert_eq!(data.agents[0].model, "sonnet");

    assert_eq!(data.agents[1].name, "bob");
    assert_eq!(data.agents[1].role, "Security Analyst");
    assert!(!data.agents[1].read_only);
    assert!(data.agents[1].model.is_empty());

    assert!(data.agents[0].prompt.contains("Code Reviewer"));
    assert!(!data.project_context.is_empty());
    assert!(data.suggested_team_id.starts_with("test-team-"));
}

#[tokio::test]
async fn test_launch_member_filter() {
    let ctx = TestContext::new().await;

    let tmp = tempfile::tempdir().unwrap();
    let agents_dir = tmp.path().join(".claude").join("agents");
    std::fs::create_dir_all(&agents_dir).unwrap();
    std::fs::write(
        agents_dir.join("filter-team.md"),
        r#"---
name: filter-team
description: Team for filter testing
---

### Alice -- Reviewer

**Focus:** Reviews

**Tools:** Read-only

### Bob -- Implementer

**Focus:** Implementation

**Tools:** Full tools

### Charlie -- Tester

**Focus:** Testing

**Tools:** Read-only
"#,
    )
    .unwrap();

    let path = tmp.path().to_str().unwrap().to_string();
    session_start(&ctx, path, None, None).await.unwrap();

    let result = handle_launch(
        &ctx,
        "filter-team".into(),
        None,
        Some("alice,charlie".into()),
        None,
    )
    .await;
    assert!(result.is_ok(), "Filtered launch should succeed");
    let data = result.unwrap().0.data.unwrap();
    assert_eq!(data.agents.len(), 2);
    assert_eq!(data.agents[0].name, "alice");
    assert_eq!(data.agents[1].name, "charlie");
}

#[tokio::test]
async fn test_launch_member_filter_no_match() {
    let ctx = TestContext::new().await;

    let tmp = tempfile::tempdir().unwrap();
    let agents_dir = tmp.path().join(".claude").join("agents");
    std::fs::create_dir_all(&agents_dir).unwrap();
    std::fs::write(
        agents_dir.join("small-team.md"),
        r#"---
name: small-team
description: Small team
---

### Alice -- Reviewer

**Focus:** Reviews

**Tools:** Read-only
"#,
    )
    .unwrap();

    let path = tmp.path().to_str().unwrap().to_string();
    session_start(&ctx, path, None, None).await.unwrap();

    let result = handle_launch(
        &ctx,
        "small-team".into(),
        None,
        Some("nonexistent".into()),
        None,
    )
    .await;
    assert!(result.is_err(), "Should fail with no matching members");
    let err = format!("{}", result.err().unwrap());
    assert!(
        err.contains("No matching members"),
        "Should mention no matching members: {}",
        err
    );
}
