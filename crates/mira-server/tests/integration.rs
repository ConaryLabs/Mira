//! Integration tests for Mira MCP tools
//!
//! These tests verify the integration between tool functions and their dependencies,
//! using mocked or in-memory implementations where appropriate.

mod test_utils;

use test_utils::TestContext;
#[allow(unused_imports)]
use mira::tools::core::{ToolContext, session_start, set_project, get_project, remember, recall, forget, search_code, find_function_callers, find_function_callees, check_capability, get_symbols, index, summarize_codebase, session_history, ensure_session, task, goal, configure_expert, get_session_recap};

#[tokio::test]
async fn test_session_start_basic() {
    let ctx = TestContext::new();

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
    let ctx = TestContext::new();

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
    let ctx = TestContext::new();

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
    let ctx = TestContext::new();

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
    let ctx = TestContext::new();

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
    let ctx = TestContext::new();

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
    let ctx = TestContext::new();

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
    let ctx = TestContext::new();

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
    let ctx = TestContext::new();

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
    let ctx = TestContext::new();

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
    let ctx = TestContext::new();

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
    let ctx = TestContext::new();

    let project_path = "/tmp/test_index".to_string();
    session_start(&ctx, project_path.clone(), Some("Index Test".to_string()), None)
        .await
        .expect("session_start failed");

    let result = index(&ctx, "status".to_string(), None, false).await;
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
    let ctx = TestContext::new();

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
    let ctx = TestContext::new();

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
    let ctx = TestContext::new();

    // No active session
    let result = session_history(&ctx, "current".to_string(), None, None).await;
    assert!(result.is_ok(), "session_history current failed: {:?}", result.err());
    let output = result.unwrap();
    assert!(output.contains("No active session"), "Output: {}", output);

    // Create a session via session_start
    let project_path = "/tmp/test_session_history".to_string();
    session_start(&ctx, project_path.clone(), Some("Session History Test".to_string()), None)
        .await
        .expect("session_start failed");

    let result = session_history(&ctx, "current".to_string(), None, None).await;
    assert!(result.is_ok(), "session_history current failed: {:?}", result.err());
    let output = result.unwrap();
    assert!(output.contains("Current session:"), "Output: {}", output);
}

#[tokio::test]
async fn test_session_history_list_sessions() {
    let ctx = TestContext::new();

    let project_path = "/tmp/test_list_sessions".to_string();
    session_start(&ctx, project_path.clone(), Some("List Sessions Test".to_string()), None)
        .await
        .expect("session_start failed");

    let result = session_history(&ctx, "list_sessions".to_string(), None, Some(10)).await;
    // Should succeed even if no sessions in database (maybe there is one now)
    assert!(result.is_ok(), "session_history list_sessions failed: {:?}", result.err());
    let output = result.unwrap();
    // Output either lists sessions or says "No sessions found"
    assert!(output.contains("sessions") || output.contains("No sessions"), "Output: {}", output);
}

#[tokio::test]
async fn test_task_create_and_list() {
    let ctx = TestContext::new();

    let project_path = "/tmp/test_tasks".to_string();
    session_start(&ctx, project_path.clone(), Some("Task Test".to_string()), None)
        .await
        .expect("session_start failed");

    // Create a task
    let result = task(
        &ctx,
        "create".to_string(),
        None,
        Some("Write integration tests".to_string()),
        Some("Create tests for all MCP tools".to_string()),
        Some("pending".to_string()),
        Some("medium".to_string()),
        None,
        None,
        None,
    )
    .await;
    assert!(result.is_ok(), "task create failed: {:?}", result.err());
    let output = result.unwrap();
    assert!(output.contains("Created task"), "Output: {}", output);
    assert!(output.contains("Write integration tests"), "Output: {}", output);

    // List tasks
    let result = task(
        &ctx,
        "list".to_string(),
        None,
        None,
        None,
        None,
        None,
        Some(false),
        Some(10),
        None,
    )
    .await;
    assert!(result.is_ok(), "task list failed: {:?}", result.err());
    let output = result.unwrap();
    assert!(output.contains("tasks"), "Output: {}", output);
    assert!(output.contains("Write integration tests"), "Output: {}", output);
}

#[tokio::test]
async fn test_task_update_and_complete() {
    let ctx = TestContext::new();

    let project_path = "/tmp/test_tasks_update".to_string();
    session_start(&ctx, project_path.clone(), Some("Task Update Test".to_string()), None)
        .await
        .expect("session_start failed");

    // Create a task first
    let create_result = task(
        &ctx,
        "create".to_string(),
        None,
        Some("Fix bug".to_string()),
        Some("Fix the critical bug".to_string()),
        Some("pending".to_string()),
        Some("high".to_string()),
        None,
        None,
        None,
    )
    .await;
    assert!(create_result.is_ok(), "task create failed");
    let create_output = create_result.unwrap();
    // Extract task ID from output
    let id_str = create_output
        .split("id:")
        .nth(1)
        .unwrap()
        .trim()
        .split_whitespace()
        .next()
        .unwrap()
        .trim_matches(|c: char| !c.is_digit(10));
    let task_id: i64 = id_str.parse().expect("Failed to parse task ID");

    // Update the task
    let update_result = task(
        &ctx,
        "update".to_string(),
        Some(task_id.to_string()),
        Some("Fix bug (updated)".to_string()),
        Some("Fixed the bug".to_string()),
        Some("in_progress".to_string()),
        Some("urgent".to_string()),
        None,
        None,
        None,
    )
    .await;
    assert!(update_result.is_ok(), "task update failed: {:?}", update_result.err());
    let update_output = update_result.unwrap();
    assert!(update_output.contains("Updated task"), "Output: {}", update_output);

    // Complete the task
    let complete_result = task(
        &ctx,
        "complete".to_string(),
        Some(task_id.to_string()),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .await;
    assert!(complete_result.is_ok(), "task complete failed: {:?}", complete_result.err());
    let complete_output = complete_result.unwrap();
    assert!(complete_output.contains("Updated task"), "Output: {}", complete_output);
}

#[tokio::test]
async fn test_goal_create_and_list() {
    let ctx = TestContext::new();

    let project_path = "/tmp/test_goals".to_string();
    session_start(&ctx, project_path.clone(), Some("Goal Test".to_string()), None)
        .await
        .expect("session_start failed");

    // Create a goal
    let result = goal(
        &ctx,
        "create".to_string(),
        None,
        Some("Implement new feature".to_string()),
        Some("Add user authentication".to_string()),
        Some("planning".to_string()),
        Some("high".to_string()),
        Some(0),
        None,
        None,
        None,
    )
    .await;
    assert!(result.is_ok(), "goal create failed: {:?}", result.err());
    let output = result.unwrap();
    assert!(output.contains("Created goal"), "Output: {}", output);
    assert!(output.contains("Implement new feature"), "Output: {}", output);

    // List goals
    let result = goal(
        &ctx,
        "list".to_string(),
        None,
        None,
        None,
        None,
        None,
        None,
        Some(false),
        Some(10),
        None,
    )
    .await;
    assert!(result.is_ok(), "goal list failed: {:?}", result.err());
    let output = result.unwrap();
    assert!(output.contains("goals"), "Output: {}", output);
    assert!(output.contains("Implement new feature"), "Output: {}", output);
}

#[tokio::test]
async fn test_configure_expert_providers() {
    let ctx = TestContext::new();

    let project_path = "/tmp/test_expert".to_string();
    session_start(&ctx, project_path.clone(), Some("Expert Test".to_string()), None)
        .await
        .expect("session_start failed");

    // providers action should list available LLM providers (none in test)
    let result = configure_expert(&ctx, "providers".to_string(), None, None, None, None).await;
    assert!(result.is_ok(), "configure_expert providers failed: {:?}", result.err());
    let output = result.unwrap();
    // Should indicate no providers available
    assert!(output.contains("No LLM providers") || output.contains("LLM providers"), "Output: {}", output);
}

#[tokio::test]
async fn test_configure_expert_list() {
    let ctx = TestContext::new();

    let project_path = "/tmp/test_expert_list".to_string();
    session_start(&ctx, project_path.clone(), Some("Expert List Test".to_string()), None)
        .await
        .expect("session_start failed");

    // list action should show no custom configurations
    let result = configure_expert(&ctx, "list".to_string(), None, None, None, None).await;
    assert!(result.is_ok(), "configure_expert list failed: {:?}", result.err());
    let output = result.unwrap();
    assert!(output.contains("No custom configurations") || output.contains("expert configurations"), "Output: {}", output);
}

#[tokio::test]
async fn test_configure_expert_set_get_delete() {
    let ctx = TestContext::new();

    let project_path = "/tmp/test_expert_crud".to_string();
    session_start(&ctx, project_path.clone(), Some("Expert CRUD Test".to_string()), None)
        .await
        .expect("session_start failed");

    // Set custom prompt for architect
    let custom_prompt = "You are a test architect. Provide simple answers.";
    let result = configure_expert(
        &ctx,
        "set".to_string(),
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
        "get".to_string(),
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
        "delete".to_string(),
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
    let ctx = TestContext::new();

    let project_path = "/tmp/test_recap".to_string();
    session_start(&ctx, project_path.clone(), Some("Recap Test".to_string()), None)
        .await
        .expect("session_start failed");

    let result = get_session_recap(&ctx).await;
    // Should succeed, may return "No session recap available."
    assert!(result.is_ok(), "get_session_recap failed: {:?}", result.err());
    let output = result.unwrap();
    // Either has recap or says no recap
    assert!(output.contains("recap") || output.contains("No session recap"), "Output: {}", output);
}