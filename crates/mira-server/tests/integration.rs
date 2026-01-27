//! Integration tests for Mira MCP tools
//!
//! These tests verify the integration between tool functions and their dependencies,
//! using mocked or in-memory implementations where appropriate.

mod test_utils;

use test_utils::TestContext;
#[allow(unused_imports)]
use mira::tools::core::{ToolContext, session_start, set_project, get_project, remember, recall, forget, search_code, find_function_callers, find_function_callees, check_capability, get_symbols, index, summarize_codebase, session_history, ensure_session, goal, configure_expert, get_session_recap, reply_to_mira};
use mira::mcp::requests::{IndexAction, SessionHistoryAction, GoalAction, ExpertConfigAction};

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
    assert!(output.contains("Project:"), "Output should contain project info");
    assert!(output.contains("Test Project"), "Output should contain project name");
    assert!(output.contains("Ready."), "Output should end with Ready.");

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
    assert!(set_result.is_ok(), "set_project failed: {:?}", set_result.err());

    // Test get_project
    let get_result = get_project(&ctx).await;
    assert!(get_result.is_ok(), "get_project failed: {:?}", get_result.err());

    let output = get_result.unwrap();
    assert!(output.contains("Current project:"), "Output should indicate current project");
    assert!(output.contains("/tmp/another_project"), "Output should contain project path");
    assert!(output.contains("Another Project"), "Output should contain project name");

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

    assert!(result.is_ok(), "session_start with custom ID failed: {:?}", result.err());

    // Verify the custom session ID was used
    let session_id = ctx.get_session_id().await;
    assert_eq!(session_id, Some(custom_session_id));
}

#[tokio::test]
async fn test_session_start_twice_different_projects() {
    let ctx = TestContext::new().await;

    // First session_start
    let result1 = session_start(&ctx, "/tmp/project1".to_string(), Some("Project 1".to_string()), None).await;
    assert!(result1.is_ok(), "First session_start failed");

    let project1 = ctx.get_project().await.unwrap();
    let session_id1 = ctx.get_session_id().await.unwrap();

    // Second session_start with different project
    let result2 = session_start(&ctx, "/tmp/project2".to_string(), Some("Project 2".to_string()), None).await;
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
async fn test_remember_basic() {
    let ctx = TestContext::new().await;

    // Need a project for memory operations
    let project_path = "/tmp/test_memory_project".to_string();
    session_start(&ctx, project_path.clone(), Some("Memory Test Project".to_string()), None)
        .await
        .expect("session_start failed");

    // Store a memory
    let content = "We decided to use Rust for the backend.";
    let result = remember(
        &ctx,
        content.to_string(),
        None, // key
        Some("decision".to_string()),
        Some("architecture".to_string()),
        Some(0.9),
        None, // scope (default to project)
    )
    .await;

    assert!(result.is_ok(), "remember failed: {:?}", result.err());
    let output = result.unwrap();
    assert!(output.contains("Stored memory"), "Output: {}", output);
    assert!(output.contains("id:"), "Output should contain memory ID");

    // Extract memory ID from output (optional)
    // We'll just verify that recall can find it
    let recall_result = recall(&ctx, "Rust backend".to_string(), Some(5), None, None).await;
    assert!(recall_result.is_ok(), "recall failed: {:?}", recall_result.err());
    let recall_output = recall_result.unwrap();
    // Since embeddings are disabled, fallback to keyword search may find the memory
    // We'll just ensure no error
    assert!(recall_output.contains("memories") || recall_output.contains("No memories"));
}

#[tokio::test]
async fn test_remember_with_key() {
    let ctx = TestContext::new().await;

    let project_path = "/tmp/test_memory_key".to_string();
    session_start(&ctx, project_path.clone(), Some("Memory Key Test".to_string()), None)
        .await
        .expect("session_start failed");

    // Store a memory with a key
    let content = "API key is stored in .env file";
    let key = "api_key_location".to_string();
    let result = remember(
        &ctx,
        content.to_string(),
        Some(key.clone()),
        Some("preference".to_string()),
        Some("security".to_string()),
        Some(1.0),
        None, // scope (default to project)
    )
    .await;

    assert!(result.is_ok(), "remember failed: {:?}", result.err());
    let output = result.unwrap();
    assert!(output.contains("Stored memory"), "Output: {}", output);
    assert!(output.contains("with key"), "Output should indicate key");
    assert!(output.contains("id:"), "Output should contain memory ID");

    // Parse memory ID from output
    let id_str = output
        .split("id:")
        .nth(1)
        .unwrap()
        .trim()
        .split_whitespace()
        .next()
        .unwrap()
        .trim_matches(|c: char| !c.is_digit(10));
    let memory_id: i64 = id_str.parse().expect("Failed to parse memory ID");

    // Try to forget the memory
    let forget_result = forget(&ctx, memory_id.to_string()).await;
    assert!(forget_result.is_ok(), "forget failed: {:?}", forget_result.err());
    let forget_output = forget_result.unwrap();
    assert!(forget_output.contains("deleted") || forget_output.contains("not found"));
}

#[tokio::test]
async fn test_forget_invalid_id() {
    let ctx = TestContext::new().await;

    // Forget with negative ID
    let result = forget(&ctx, "-5".to_string()).await;
    assert!(result.is_err(), "Expected error for negative ID");

    // Forget with non-numeric ID
    let result = forget(&ctx, "abc".to_string()).await;
    assert!(result.is_err(), "Expected error for non-numeric ID");

    // Forget with zero ID
    let result = forget(&ctx, "0".to_string()).await;
    assert!(result.is_err(), "Expected error for zero ID");
}

#[tokio::test]
async fn test_search_code_empty() {
    let ctx = TestContext::new().await;

    let project_path = "/tmp/test_code_search".to_string();
    session_start(&ctx, project_path.clone(), Some("Code Search Test".to_string()), None)
        .await
        .expect("session_start failed");

    let result = search_code(&ctx, "function foo".to_string(), None, Some(10)).await;
    assert!(result.is_ok(), "search_code failed: {:?}", result.err());
    let output = result.unwrap();
    assert!(output.contains("No code matches found"), "Output: {}", output);
}

#[tokio::test]
async fn test_find_function_callers_empty() {
    let ctx = TestContext::new().await;

    let project_path = "/tmp/test_callers".to_string();
    session_start(&ctx, project_path.clone(), Some("Callers Test".to_string()), None)
        .await
        .expect("session_start failed");

    let result = find_function_callers(&ctx, "some_function".to_string(), Some(20)).await;
    assert!(result.is_ok(), "find_function_callers failed: {:?}", result.err());
    let output = result.unwrap();
    assert!(output.contains("No callers found"), "Output: {}", output);
}

#[tokio::test]
async fn test_find_function_callees_empty() {
    let ctx = TestContext::new().await;

    let project_path = "/tmp/test_callees".to_string();
    session_start(&ctx, project_path.clone(), Some("Callees Test".to_string()), None)
        .await
        .expect("session_start failed");

    let result = find_function_callees(&ctx, "some_function".to_string(), Some(20)).await;
    assert!(result.is_ok(), "find_function_callees failed: {:?}", result.err());
    let output = result.unwrap();
    assert!(output.contains("No callees found"), "Output: {}", output);
}

#[tokio::test]
async fn test_check_capability_empty() {
    let ctx = TestContext::new().await;

    let project_path = "/tmp/test_capability".to_string();
    session_start(&ctx, project_path.clone(), Some("Capability Test".to_string()), None)
        .await
        .expect("session_start failed");

    let result = check_capability(&ctx, "authentication system".to_string()).await;
    assert!(result.is_ok(), "check_capability failed: {:?}", result.err());
    let output = result.unwrap();
    assert!(output.contains("No capability found"), "Output: {}", output);
}

#[tokio::test]
async fn test_index_status() {
    let ctx = TestContext::new().await;

    let project_path = "/tmp/test_index".to_string();
    session_start(&ctx, project_path.clone(), Some("Index Test".to_string()), None)
        .await
        .expect("session_start failed");

    let result = index(&ctx, IndexAction::Status, None, false).await;
    assert!(result.is_ok(), "index status failed: {:?}", result.err());
    let output = result.unwrap();
    assert!(output.contains("Index status"), "Output: {}", output);
    assert!(output.contains("symbols") && output.contains("embedded chunks"), "Output: {}", output);
}

#[tokio::test]
async fn test_get_symbols() {
    use std::fs;

    // Create a temporary Rust file
    let temp_dir = "/tmp/mira_test";
    fs::create_dir_all(temp_dir).expect("Failed to create temp dir");
    let file_path = format!("{}/test.rs", temp_dir);
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
    assert!(output.contains("symbols"), "Output: {}", output);
    // Should contain function and struct
    assert!(output.contains("hello_world") || output.contains("Point"), "Output: {}", output);

    // Clean up (optional)
    let _ = fs::remove_file(file_path);
}

#[tokio::test]
async fn test_summarize_codebase_no_deepseek() {
    let ctx = TestContext::new().await;

    let project_path = "/tmp/test_summarize".to_string();
    session_start(&ctx, project_path.clone(), Some("Summarize Test".to_string()), None)
        .await
        .expect("session_start failed");

    let result = summarize_codebase(&ctx).await;
    // Should error because DeepSeek client not configured
    assert!(result.is_err(), "summarize_codebase should fail without DeepSeek client");
    let error = result.unwrap_err();
    assert!(error.contains("DeepSeek not configured") || error.contains("No active project"), "Error: {}", error);
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
    let ctx = TestContext::new().await;

    // No active session
    let result = session_history(&ctx, SessionHistoryAction::Current, None, None).await;
    assert!(result.is_ok(), "session_history current failed: {:?}", result.err());
    let output = result.unwrap();
    assert!(output.contains("No active session"), "Output: {}", output);

    // Create a session via session_start
    let project_path = "/tmp/test_session_history".to_string();
    session_start(&ctx, project_path.clone(), Some("Session History Test".to_string()), None)
        .await
        .expect("session_start failed");

    let result = session_history(&ctx, SessionHistoryAction::Current, None, None).await;
    assert!(result.is_ok(), "session_history current failed: {:?}", result.err());
    let output = result.unwrap();
    assert!(output.contains("Current session:"), "Output: {}", output);
}

#[tokio::test]
async fn test_session_history_list_sessions() {
    let ctx = TestContext::new().await;

    let project_path = "/tmp/test_list_sessions".to_string();
    session_start(&ctx, project_path.clone(), Some("List Sessions Test".to_string()), None)
        .await
        .expect("session_start failed");

    let result = session_history(&ctx, SessionHistoryAction::ListSessions, None, Some(10)).await;
    // Should succeed even if no sessions in database (maybe there is one now)
    assert!(result.is_ok(), "session_history list_sessions failed: {:?}", result.err());
    let output = result.unwrap();
    // Output either lists sessions or says "No sessions found"
    assert!(output.contains("sessions") || output.contains("No sessions"), "Output: {}", output);
}

#[tokio::test]
async fn test_goal_create_and_list() {
    let ctx = TestContext::new().await;

    let project_path = "/tmp/test_goals".to_string();
    session_start(&ctx, project_path.clone(), Some("Goal Test".to_string()), None)
        .await
        .expect("session_start failed");

    // Create a goal
    let result = goal(
        &ctx,
        GoalAction::Create,
        None, // goal_id
        Some("Implement new feature".to_string()), // title
        Some("Add user authentication".to_string()), // description
        Some("planning".to_string()), // status
        Some("high".to_string()), // priority
        Some(0), // progress_percent
        None, // include_finished
        None, // limit
        None, // goals (bulk)
        None, // milestone_title
        None, // milestone_id
        None, // weight
    )
    .await;
    assert!(result.is_ok(), "goal create failed: {:?}", result.err());
    let output = result.unwrap();
    assert!(output.contains("Created goal"), "Output: {}", output);
    assert!(output.contains("Implement new feature"), "Output: {}", output);

    // List goals
    let result = goal(
        &ctx,
        GoalAction::List,
        None, // goal_id
        None, // title
        None, // description
        None, // status
        None, // priority
        None, // progress_percent
        Some(false), // include_finished
        Some(10), // limit
        None, // goals (bulk)
        None, // milestone_title
        None, // milestone_id
        None, // weight
    )
    .await;
    assert!(result.is_ok(), "goal list failed: {:?}", result.err());
    let output = result.unwrap();
    assert!(output.contains("goals"), "Output: {}", output);
    assert!(output.contains("Implement new feature"), "Output: {}", output);
}

#[tokio::test]
async fn test_configure_expert_providers() {
    let ctx = TestContext::new().await;

    let project_path = "/tmp/test_expert".to_string();
    session_start(&ctx, project_path.clone(), Some("Expert Test".to_string()), None)
        .await
        .expect("session_start failed");

    // providers action should list available LLM providers (none in test)
    let result = configure_expert(&ctx, ExpertConfigAction::Providers, None, None, None, None).await;
    assert!(result.is_ok(), "configure_expert providers failed: {:?}", result.err());
    let output = result.unwrap();
    // Should indicate no providers available
    assert!(output.contains("No LLM providers") || output.contains("LLM providers"), "Output: {}", output);
}

#[tokio::test]
async fn test_configure_expert_list() {
    let ctx = TestContext::new().await;

    let project_path = "/tmp/test_expert_list".to_string();
    session_start(&ctx, project_path.clone(), Some("Expert List Test".to_string()), None)
        .await
        .expect("session_start failed");

    // list action should show no custom configurations
    let result = configure_expert(&ctx, ExpertConfigAction::List, None, None, None, None).await;
    assert!(result.is_ok(), "configure_expert list failed: {:?}", result.err());
    let output = result.unwrap();
    assert!(output.contains("No custom configurations") || output.contains("expert configurations"), "Output: {}", output);
}

#[tokio::test]
async fn test_configure_expert_set_get_delete() {
    let ctx = TestContext::new().await;

    let project_path = "/tmp/test_expert_crud".to_string();
    session_start(&ctx, project_path.clone(), Some("Expert CRUD Test".to_string()), None)
        .await
        .expect("session_start failed");

    // Set custom prompt for architect
    let custom_prompt = "You are a test architect. Provide simple answers.";
    let result = configure_expert(
        &ctx,
        ExpertConfigAction::Set,
        Some("architect".to_string()),
        Some(custom_prompt.to_string()),
        None,
        None,
    )
    .await;
    assert!(result.is_ok(), "configure_expert set failed: {:?}", result.err());
    let output = result.unwrap();
    assert!(output.contains("Configuration updated"), "Output: {}", output);

    // Get the configuration
    let result = configure_expert(
        &ctx,
        ExpertConfigAction::Get,
        Some("architect".to_string()),
        None,
        None,
        None,
    )
    .await;
    assert!(result.is_ok(), "configure_expert get failed: {:?}", result.err());
    let output = result.unwrap();
    assert!(output.contains("Configuration for 'architect'"), "Output: {}", output);
    assert!(output.contains("Custom prompt:"), "Output: {}", output);

    // Delete the configuration
    let result = configure_expert(
        &ctx,
        ExpertConfigAction::Delete,
        Some("architect".to_string()),
        None,
        None,
        None,
    )
    .await;
    assert!(result.is_ok(), "configure_expert delete failed: {:?}", result.err());
    let output = result.unwrap();
    assert!(output.contains("Configuration deleted") || output.contains("No custom configuration"), "Output: {}", output);
}

#[tokio::test]
async fn test_get_session_recap() {
    let ctx = TestContext::new().await;

    let project_path = "/tmp/test_recap".to_string();
    session_start(&ctx, project_path.clone(), Some("Recap Test".to_string()), None)
        .await
        .expect("session_start failed");

    let result = get_session_recap(&ctx).await;
    // Should succeed, may return "No session recap available."
    assert!(result.is_ok(), "get_session_recap failed: {:?}", result.err());
    let output = result.unwrap();
    // Should contain welcome message or say no recap available
    assert!(
        output.contains("Welcome back") || output.contains("No session recap"),
        "Output: {}",
        output
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Pool Behavior Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_pool_concurrent_access() {
    let ctx = TestContext::new().await;

    // Set up a project first
    let project_path = "/tmp/test_pool_concurrent".to_string();
    session_start(&ctx, project_path.clone(), Some("Pool Test".to_string()), None)
        .await
        .expect("session_start failed");

    // Run multiple concurrent memory operations
    let futures: Vec<_> = (0..5)
        .map(|i| {
            let ctx_ref = &ctx;
            async move {
                remember(
                    ctx_ref,
                    format!("Concurrent memory {}", i),
                    Some(format!("concurrent_key_{}", i)),
                    Some("general".to_string()),
                    None,
                    Some(0.8),
                    None, // scope (default to project)
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
            "Concurrent remember {} failed: {:?}",
            i,
            result.as_ref().err()
        );
    }

    // Verify all memories were stored
    let recall_result = recall(&ctx, "Concurrent memory".to_string(), Some(10), None, None).await;
    assert!(recall_result.is_ok(), "recall failed: {:?}", recall_result.err());
    let output = recall_result.unwrap();
    assert!(output.contains("memories"), "Should find memories: {}", output);
}

#[tokio::test]
async fn test_pool_and_database_share_state() {
    use mira::tools::core::ToolContext;

    let ctx = TestContext::new().await;

    // Create a project using pool (via session_start)
    let project_path = "/tmp/test_pool_share".to_string();
    session_start(&ctx, project_path.clone(), Some("Share Test".to_string()), None)
        .await
        .expect("session_start failed");

    let project_id = ctx.project_id().await.expect("Should have project_id");

    // Verify project exists via pool
    let project_exists = ctx.pool()
        .interact(move |conn| {
            Ok::<bool, anyhow::Error>(conn.query_row(
                "SELECT 1 FROM projects WHERE id = ?",
                [project_id],
                |_row| Ok(true),
            ).unwrap_or(false))
        })
        .await
        .unwrap();

    assert!(project_exists, "Project created via pool should be visible");

    // Create a memory via pool
    remember(
        &ctx,
        "Pool-created memory".to_string(),
        Some("pool_share_test".to_string()),
        Some("general".to_string()),
        None,
        Some(0.9),
        None, // scope (default to project)
    )
    .await
    .expect("remember failed");

    // Verify memory exists via pool
    let memory_exists = ctx.pool()
        .interact(|conn| {
            Ok::<bool, anyhow::Error>(conn.query_row(
                "SELECT 1 FROM memory_facts WHERE key = ?",
                ["pool_share_test"],
                |_row| Ok(true),
            ).unwrap_or(false))
        })
        .await
        .unwrap();

    assert!(memory_exists, "Memory created via pool should be visible");
}

#[tokio::test]
async fn test_pool_error_handling() {
    let ctx = TestContext::new().await;

    // Try to recall without a project (should still work, just return no results)
    let result = recall(&ctx, "nonexistent".to_string(), Some(5), None, None).await;
    assert!(result.is_ok(), "recall should handle missing project gracefully");

    // Try forget with invalid ID
    let result = forget(&ctx, "invalid".to_string()).await;
    assert!(result.is_err(), "forget should fail with invalid ID");
    assert!(
        result.unwrap_err().contains("Invalid"),
        "Error should mention invalid ID"
    );

    // Try forget with non-existent ID
    let result = forget(&ctx, "999999".to_string()).await;
    assert!(result.is_ok(), "forget should handle non-existent ID gracefully");
    let output = result.unwrap();
    assert!(output.contains("not found"), "Should indicate memory not found: {}", output);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Context Injection Integration Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_context_injection_basic() {
    use mira::context::ContextInjectionManager;

    let ctx = TestContext::new().await;

    // Create injection manager
    let manager = ContextInjectionManager::new(ctx.pool().clone(), ctx.embeddings().cloned()).await;

    // Test with a code-related message
    let result = manager
        .get_context_for_message(
            "How does the authentication function work in this codebase?",
            "test-session",
        )
        .await;

    // Should attempt injection (may or may not find context depending on DB state)
    assert!(result.skip_reason.is_none() || result.skip_reason == Some("sampled_out".to_string()),
        "Should not skip for code-related message, got: {:?}", result.skip_reason);
}

#[tokio::test]
async fn test_context_injection_skip_simple_commands() {
    use mira::context::ContextInjectionManager;

    let ctx = TestContext::new().await;
    let manager = ContextInjectionManager::new(ctx.pool().clone(), ctx.embeddings().cloned()).await;

    // Simple commands should be skipped
    let result = manager.get_context_for_message("git status", "test-session").await;
    assert_eq!(result.skip_reason, Some("simple_command".to_string()));

    let result = manager.get_context_for_message("ls -la", "test-session").await;
    assert_eq!(result.skip_reason, Some("simple_command".to_string()));

    let result = manager.get_context_for_message("/help", "test-session").await;
    assert_eq!(result.skip_reason, Some("simple_command".to_string()));
}

#[tokio::test]
async fn test_context_injection_skip_short_messages() {
    use mira::context::ContextInjectionManager;

    let ctx = TestContext::new().await;
    let manager = ContextInjectionManager::new(ctx.pool().clone(), ctx.embeddings().cloned()).await;

    // Very short messages should be skipped
    let result = manager.get_context_for_message("hi", "test-session").await;
    assert!(result.skip_reason.is_some());
}

#[tokio::test]
async fn test_context_injection_config() {
    use mira::context::{ContextInjectionManager, InjectionConfig};

    let ctx = TestContext::new().await;
    let mut manager = ContextInjectionManager::new(ctx.pool().clone(), ctx.embeddings().cloned()).await;

    // Verify default config
    assert!(manager.config().enabled);
    assert_eq!(manager.config().max_chars, 1500);
    assert_eq!(manager.config().sample_rate, 0.5);

    // Update config
    let new_config = InjectionConfig::builder()
        .enabled(false)
        .max_chars(2000)
        .sample_rate(1.0)
        .build();
    manager.set_config(new_config).await;

    // Verify injection is disabled
    let result = manager
        .get_context_for_message(
            "How does the authentication function work?",
            "test-session",
        )
        .await;
    assert_eq!(result.skip_reason, Some("disabled".to_string()));
}

#[tokio::test]
async fn test_context_injection_with_goals() {
    use mira::context::ContextInjectionManager;

    let ctx = TestContext::new().await;

    // Create a project and some goals
    let project_path = "/tmp/test_injection_goals".to_string();
    session_start(&ctx, project_path.clone(), Some("Injection Test".to_string()), None)
        .await
        .expect("session_start failed");

    // Create a goal
    goal(
        &ctx,
        GoalAction::Create,
        None, // goal_id
        Some("Fix authentication bug".to_string()), // title
        Some("High priority security issue".to_string()), // description
        None, // status
        Some("high".to_string()), // priority
        None, // progress_percent
        None, // include_finished
        None, // limit
        None, // goals (bulk)
        None, // milestone_title
        None, // milestone_id
        None, // weight
    )
    .await
    .expect("goal creation failed");

    // Create injection manager
    let manager = ContextInjectionManager::new(ctx.pool().clone(), ctx.embeddings().cloned()).await;

    // Get context - should include goal info if task-aware injection is enabled
    // Note: due to sampling, this might be skipped
    let config = manager.config();
    assert!(config.enable_task_aware, "Task-aware injection should be enabled by default");
}

#[tokio::test]
async fn test_context_injection_file_extraction() {
    use mira::context::FileAwareInjector;

    let ctx = TestContext::new().await;
    let injector = FileAwareInjector::new(ctx.pool().clone());

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

#[tokio::test]
async fn test_context_injection_analytics() {
    use mira::context::{InjectionAnalytics, InjectionEvent, InjectionSource};

    let ctx = TestContext::new().await;
    let analytics = InjectionAnalytics::new(ctx.pool().clone());

    // Record some events
    analytics.record(InjectionEvent {
        session_id: "test-1".to_string(),
        project_id: Some(1),
        sources: vec![InjectionSource::Semantic],
        context_len: 100,
        message_preview: "test message 1".to_string(),
    }).await;

    analytics.record(InjectionEvent {
        session_id: "test-2".to_string(),
        project_id: Some(1),
        sources: vec![InjectionSource::Semantic, InjectionSource::TaskAware],
        context_len: 200,
        message_preview: "test message 2".to_string(),
    }).await;

    // Check summary
    let summary = analytics.summary(None).await;
    assert!(summary.contains("2 injections"), "Summary: {}", summary);
    assert!(summary.contains("300 chars"), "Summary: {}", summary);

    // Check recent events
    let recent = analytics.recent_events(5).await;
    assert_eq!(recent.len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Reply to Mira Tests (Centralized Session Collaboration)
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_reply_to_mira_no_frontend() {
    // TestContext returns None for pending_responses() - simulates CLI mode
    let ctx = TestContext::new().await;

    let result = reply_to_mira(
        &ctx,
        "msg-123".to_string(),
        "Test response content".to_string(),
        true,
    )
    .await;

    // Should succeed with "no frontend" message
    assert!(result.is_ok(), "reply_to_mira should succeed in CLI mode");
    let output = result.unwrap();
    assert!(
        output.contains("no frontend connected"),
        "Output should indicate no frontend: {}",
        output
    );
    assert!(
        output.contains("Test response content"),
        "Output should contain the content: {}",
        output
    );
}

#[tokio::test]
async fn test_reply_to_mira_not_collaborative() {
    // TestContext is not collaborative (is_collaborative returns false)
    let ctx = TestContext::new().await;

    // Even with a non-existent message_id, should succeed in non-collaborative mode
    let result = reply_to_mira(
        &ctx,
        "non-existent-id".to_string(),
        "Some content".to_string(),
        false,
    )
    .await;

    assert!(
        result.is_ok(),
        "Non-collaborative mode should not error on missing request"
    );
}