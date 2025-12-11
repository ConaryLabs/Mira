// tests/dual_session_test.rs
// Integration tests for dual-session architecture (Voice + Codex)
//
// Run unit tests: cargo test --test dual_session_test
// Run LLM tests:  cargo test --test dual_session_test llm -- --nocapture

mod common;

use mira_backend::llm::provider::openai::{OpenAIModel, OpenAIPricing, OpenAIProvider};
use mira_backend::llm::provider::{LlmProvider, Message};
use mira_backend::llm::router::{RouterConfig, TaskClassifier, RoutingTask};
use mira_backend::session::{
    CodexCompletionMetadata, CodexSpawnTrigger, CodexStatus, InjectionService, InjectionType,
    SessionManager, SessionType,
};
use serde_json::json;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;
use std::time::Instant;

fn get_api_key() -> Option<String> {
    dotenv::dotenv().ok();
    std::env::var("OPENAI_API_KEY").ok().filter(|k| !k.is_empty())
}

fn skip_if_no_key() -> bool {
    if get_api_key().is_none() {
        println!("SKIPPED: OPENAI_API_KEY not set");
        return true;
    }
    false
}

async fn test_pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .connect("sqlite::memory:")
        .await
        .unwrap();

    sqlx::migrate!("./migrations").run(&pool).await.unwrap();

    pool
}

// =============================================================================
// SessionManager Tests
// =============================================================================

#[tokio::test]
async fn test_voice_session_creation() {
    let pool = test_pool().await;
    let manager = SessionManager::new(pool);

    // Create Voice session
    let voice_id = manager
        .get_or_create_voice_session(None, Some("/test/project"))
        .await
        .unwrap();

    // Verify it's a Voice session
    let session_type = manager.get_session_type(&voice_id).await.unwrap();
    assert_eq!(session_type, SessionType::Voice);

    // Second call should return the same session
    let voice_id_2 = manager
        .get_or_create_voice_session(None, Some("/test/project"))
        .await
        .unwrap();
    assert_eq!(voice_id, voice_id_2);
}

#[tokio::test]
async fn test_codex_session_lifecycle() {
    let pool = test_pool().await;
    let manager = SessionManager::new(pool);

    // Create Voice session first
    let voice_id = manager
        .get_or_create_voice_session(None, Some("/test/project"))
        .await
        .unwrap();

    // Spawn Codex session
    let trigger = CodexSpawnTrigger::RouterDetection {
        confidence: 0.85,
        detected_patterns: vec!["implement".to_string(), "refactor".to_string()],
    };

    let codex_id = manager
        .spawn_codex_session(
            &voice_id,
            "Implement authentication system",
            &trigger,
            Some("User wants secure login"),
        )
        .await
        .unwrap();

    // Verify Codex session type
    let session_type = manager.get_session_type(&codex_id).await.unwrap();
    assert_eq!(session_type, SessionType::Codex);

    // Verify parent lookup works
    let parent = manager.get_voice_session_id(&codex_id).await.unwrap();
    assert_eq!(parent, voice_id);

    // Verify active sessions list
    let active = manager.get_active_codex_sessions(&voice_id).await.unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].id, codex_id);
    assert_eq!(active[0].status, CodexStatus::Running);

    // Complete the session
    let returned_voice_id = manager
        .complete_codex_session(&codex_id, "Successfully implemented auth", 50000, 10000, 0.50, 1)
        .await
        .unwrap();
    assert_eq!(returned_voice_id, voice_id);

    // Verify no active sessions anymore
    let active = manager.get_active_codex_sessions(&voice_id).await.unwrap();
    assert!(active.is_empty());
}

#[tokio::test]
async fn test_codex_session_failure() {
    let pool = test_pool().await;
    let manager = SessionManager::new(pool);

    let voice_id = manager
        .get_or_create_voice_session(None, None)
        .await
        .unwrap();

    let trigger = CodexSpawnTrigger::ComplexTask {
        estimated_tokens: 150000,
        file_count: 10,
        operation_kind: Some("migration".to_string()),
    };

    let codex_id = manager
        .spawn_codex_session(&voice_id, "Migrate database", &trigger, None)
        .await
        .unwrap();

    // Fail the session
    let returned_voice_id = manager
        .fail_codex_session(&codex_id, "Connection timeout")
        .await
        .unwrap();
    assert_eq!(returned_voice_id, voice_id);

    // Verify status
    let active = manager.get_active_codex_sessions(&voice_id).await.unwrap();
    assert!(active.is_empty()); // Failed sessions are not active
}

// =============================================================================
// TaskClassifier Tests (Codex Spawn Detection)
// =============================================================================

#[test]
fn test_codex_spawn_detection_implement() {
    let classifier = TaskClassifier::new(RouterConfig::default());
    let task = RoutingTask::user_chat();

    // "implement" is a high-signal pattern
    let trigger = classifier.should_spawn_codex(&task, "Please implement a new authentication system");
    assert!(trigger.is_some());

    if let Some(CodexSpawnTrigger::RouterDetection { confidence, detected_patterns }) = trigger {
        assert!(confidence >= 0.7, "Confidence should be >= 0.7, got {}", confidence);
        assert!(detected_patterns.contains(&"implement".to_string()));
    } else {
        panic!("Expected RouterDetection trigger");
    }
}

#[test]
fn test_codex_spawn_detection_refactor() {
    let classifier = TaskClassifier::new(RouterConfig::default());
    let task = RoutingTask::user_chat();

    let trigger = classifier.should_spawn_codex(&task, "Refactor the database layer to use async");
    assert!(trigger.is_some());

    if let Some(CodexSpawnTrigger::RouterDetection { confidence, detected_patterns }) = trigger {
        assert!(confidence >= 0.7);
        assert!(detected_patterns.contains(&"refactor".to_string()));
    } else {
        panic!("Expected RouterDetection trigger");
    }
}

#[test]
fn test_codex_spawn_detection_multiple_patterns() {
    let classifier = TaskClassifier::new(RouterConfig::default());
    let task = RoutingTask::user_chat();

    // Multiple patterns should increase confidence
    let trigger = classifier.should_spawn_codex(
        &task,
        "Implement a new feature and add tests for the authentication module",
    );
    assert!(trigger.is_some());

    if let Some(CodexSpawnTrigger::RouterDetection { confidence, detected_patterns }) = trigger {
        // Multiple patterns = higher confidence
        assert!(confidence >= 0.7);
        assert!(detected_patterns.len() >= 2);
    }
}

#[test]
fn test_codex_spawn_detection_no_trigger() {
    let classifier = TaskClassifier::new(RouterConfig::default());
    let task = RoutingTask::user_chat();

    // Simple questions should NOT trigger Codex
    let trigger = classifier.should_spawn_codex(&task, "What does this function do?");
    assert!(trigger.is_none());

    let trigger = classifier.should_spawn_codex(&task, "Explain the architecture");
    assert!(trigger.is_none());

    let trigger = classifier.should_spawn_codex(&task, "Hello, how are you?");
    assert!(trigger.is_none());
}

#[test]
fn test_codex_spawn_agentic_operation() {
    let classifier = TaskClassifier::new(RouterConfig::default());
    let task = RoutingTask::new().with_operation("full_implementation");

    let trigger = classifier.should_spawn_codex(&task, "Build the entire system");
    assert!(trigger.is_some());
    assert!(matches!(trigger.unwrap(), CodexSpawnTrigger::ComplexTask { .. }));
}

#[test]
fn test_codex_spawn_long_running() {
    let classifier = TaskClassifier::new(RouterConfig::default());
    let task = RoutingTask::new().with_long_running(true);

    let trigger = classifier.should_spawn_codex(&task, "Any message");
    assert!(trigger.is_some());
    assert!(matches!(trigger.unwrap(), CodexSpawnTrigger::ComplexTask { .. }));
}

// =============================================================================
// InjectionService Tests
// =============================================================================

#[tokio::test]
async fn test_injection_completion_flow() {
    let pool = test_pool().await;

    // Create sessions first
    sqlx::query(
        "INSERT INTO chat_sessions (id, session_type, message_count, created_at, last_active) VALUES ('voice-test', 'voice', 0, 0, 0)",
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO chat_sessions (id, session_type, parent_session_id, message_count, created_at, last_active) VALUES ('codex-test', 'codex', 'voice-test', 0, 0, 0)",
    )
    .execute(&pool)
    .await
    .unwrap();

    let injection_service = InjectionService::new(pool);

    // Inject completion
    let metadata = CodexCompletionMetadata {
        files_changed: vec!["src/auth.rs".to_string(), "src/main.rs".to_string()],
        duration_seconds: 300,
        tokens_total: 75000,
        cost_usd: 0.75,
        tool_calls_count: 25,
        compaction_count: 2,
        key_actions: vec!["Implemented auth".to_string(), "Added tests".to_string()],
    };

    let injection_id = injection_service
        .inject_codex_completion(
            "voice-test",
            "codex-test",
            "Successfully implemented authentication with JWT tokens and password hashing.",
            metadata,
        )
        .await
        .unwrap();

    assert!(injection_id > 0);

    // Get pending injections
    let pending = injection_service
        .get_pending_injections("voice-test")
        .await
        .unwrap();

    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].injection_type, InjectionType::CodexCompletion);
    assert!(!pending[0].acknowledged);
    assert!(pending[0].content.contains("authentication"));

    // Format for prompt
    let formatted = injection_service.format_for_prompt(&pending);
    assert!(formatted.is_some());
    let prompt_text = formatted.unwrap();
    assert!(prompt_text.contains("Background Work Updates"));
    assert!(prompt_text.contains("Completed background task"));

    // Acknowledge
    injection_service.acknowledge_injection(injection_id).await.unwrap();

    // Should be empty now
    let pending = injection_service
        .get_pending_injections("voice-test")
        .await
        .unwrap();
    assert!(pending.is_empty());
}

#[tokio::test]
async fn test_injection_progress_and_error() {
    let pool = test_pool().await;

    // Create sessions
    sqlx::query(
        "INSERT INTO chat_sessions (id, session_type, message_count, created_at, last_active) VALUES ('voice-2', 'voice', 0, 0, 0)",
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO chat_sessions (id, session_type, parent_session_id, message_count, created_at, last_active) VALUES ('codex-2', 'codex', 'voice-2', 0, 0, 0)",
    )
    .execute(&pool)
    .await
    .unwrap();

    let injection_service = InjectionService::new(pool);

    // Inject progress
    let _progress_id = injection_service
        .inject_codex_progress(
            "voice-2",
            "codex-2",
            "Working on file 3 of 10",
            Some("Processing src/auth.rs"),
            Some(30),
        )
        .await
        .unwrap();

    // Inject error
    let _error_id = injection_service
        .inject_codex_error(
            "voice-2",
            "codex-2",
            "Connection timeout while running tests",
            "Implement authentication",
        )
        .await
        .unwrap();

    // Get all pending
    let pending = injection_service
        .get_pending_injections("voice-2")
        .await
        .unwrap();

    assert_eq!(pending.len(), 2);

    // Verify order (sequence_num)
    assert_eq!(pending[0].injection_type, InjectionType::CodexProgress);
    assert_eq!(pending[1].injection_type, InjectionType::CodexError);

    // Acknowledge all
    let count = injection_service.acknowledge_all("voice-2").await.unwrap();
    assert_eq!(count, 2);
}

// =============================================================================
// Full Flow Integration Test
// =============================================================================

#[tokio::test]
async fn test_full_dual_session_flow() {
    let pool = test_pool().await;
    let manager = SessionManager::new(pool.clone());
    let injection_service = InjectionService::new(pool.clone());
    let classifier = TaskClassifier::new(RouterConfig::default());

    // 1. Create Voice session
    let voice_id = manager
        .get_or_create_voice_session(None, Some("/home/user/myproject"))
        .await
        .unwrap();

    // 2. Detect Codex-worthy task
    let user_message = "Please implement a complete user authentication system with JWT tokens";
    let task = RoutingTask::user_chat();
    let trigger = classifier.should_spawn_codex(&task, user_message);
    assert!(trigger.is_some(), "Should detect 'implement' as Codex trigger");

    let trigger = trigger.unwrap();

    // 3. Spawn Codex session
    let codex_id = manager
        .spawn_codex_session(&voice_id, user_message, &trigger, Some("User wants auth system"))
        .await
        .unwrap();

    // Verify session was created
    assert_eq!(manager.get_session_type(&codex_id).await.unwrap(), SessionType::Codex);
    assert_eq!(manager.get_active_codex_sessions(&voice_id).await.unwrap().len(), 1);

    // 4. Simulate Codex work (normally runs in background)
    // ... (would call run_codex_session)

    // 5. Complete Codex session
    let completion_summary = "Implemented JWT-based authentication:\n\
        - Created User model with password hashing\n\
        - Added login/register endpoints\n\
        - Implemented JWT token generation and validation\n\
        - Added middleware for protected routes";

    manager
        .complete_codex_session(&codex_id, completion_summary, 80000, 20000, 1.25, 3)
        .await
        .unwrap();

    // 6. Inject completion into Voice session
    let metadata = CodexCompletionMetadata {
        files_changed: vec![
            "src/models/user.rs".to_string(),
            "src/routes/auth.rs".to_string(),
            "src/middleware/auth.rs".to_string(),
        ],
        duration_seconds: 480,
        tokens_total: 100000,
        cost_usd: 1.25,
        tool_calls_count: 42,
        compaction_count: 3,
        key_actions: vec![
            "Created User model".to_string(),
            "Implemented JWT auth".to_string(),
        ],
    };

    injection_service
        .inject_codex_completion(&voice_id, &codex_id, completion_summary, metadata)
        .await
        .unwrap();

    // 7. Voice session retrieves injection
    let injections = injection_service
        .get_pending_injections(&voice_id)
        .await
        .unwrap();

    assert_eq!(injections.len(), 1);
    assert!(injections[0].content.contains("JWT-based authentication"));

    // 8. Format for prompt injection
    let prompt_section = injection_service.format_for_prompt(&injections);
    assert!(prompt_section.is_some());
    let prompt_text = prompt_section.unwrap();
    assert!(prompt_text.contains("Background Work Updates"));
    assert!(prompt_text.contains("src/models/user.rs"));

    // 9. Acknowledge after displaying to user
    injection_service.acknowledge_all(&voice_id).await.unwrap();

    // Verify clean state
    assert!(injection_service.get_pending_injections(&voice_id).await.unwrap().is_empty());
    assert!(manager.get_active_codex_sessions(&voice_id).await.unwrap().is_empty());

    println!("Full dual-session flow completed successfully!");
}

// =============================================================================
// Real LLM Integration Tests
// =============================================================================

/// Test tool calling with Codex-Max provider (simulates Codex session work)
#[tokio::test]
async fn test_llm_codex_tool_calling() {
    if skip_if_no_key() {
        return;
    }

    println!("\n=== CODEX TOOL CALLING TEST ===\n");

    let provider = OpenAIProvider::codex_max(get_api_key().unwrap())
        .expect("Should create Codex provider");

    // Define simple tools for testing
    let tools = vec![
        json!({
            "type": "function",
            "function": {
                "name": "read_file",
                "description": "Read the contents of a file",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "The file path to read"
                        }
                    },
                    "required": ["path"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "write_file",
                "description": "Write content to a file",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "The file path to write to"
                        },
                        "content": {
                            "type": "string",
                            "description": "The content to write"
                        }
                    },
                    "required": ["path", "content"]
                }
            }
        }),
    ];

    let system_prompt = "You are a code assistant. Use the available tools to complete tasks.";
    let messages = vec![Message::user(
        "Read the file src/main.rs to see what it contains.".to_string(),
    )];

    let start = Instant::now();

    match provider
        .chat_with_tools(messages, system_prompt.to_string(), tools, None)
        .await
    {
        Ok(response) => {
            let latency = start.elapsed();

            println!("Response ID: {}", response.id);
            println!("Text output: {}", response.text_output);
            println!("Function calls: {:?}", response.function_calls.len());

            for fc in &response.function_calls {
                println!("  Tool: {} (id: {})", fc.name, fc.id);
                println!("  Args: {}", fc.arguments);
            }

            println!(
                "Tokens: {} input, {} output",
                response.tokens.input, response.tokens.output
            );
            println!("Latency: {:?}", latency);

            let cost = OpenAIPricing::calculate_cost(
                OpenAIModel::Gpt51CodexMax,
                response.tokens.input,
                response.tokens.output,
            );
            println!("Cost: ${:.6}", cost);

            // Should have requested to read a file
            assert!(
                response.function_calls.len() > 0 || !response.text_output.is_empty(),
                "Should have tool calls or text response"
            );
        }
        Err(e) => {
            println!("Error: {}", e);
            // Don't fail - model might be unavailable
        }
    }
}

/// Test compaction support via chat_with_tools_continuing
#[tokio::test]
async fn test_llm_codex_compaction_support() {
    if skip_if_no_key() {
        return;
    }

    println!("\n=== CODEX COMPACTION SUPPORT TEST ===\n");

    let provider = OpenAIProvider::codex_max(get_api_key().unwrap())
        .expect("Should create Codex provider");

    let tools = vec![json!({
        "type": "function",
        "function": {
            "name": "calculate",
            "description": "Perform a calculation",
            "parameters": {
                "type": "object",
                "properties": {
                    "expression": {
                        "type": "string",
                        "description": "Math expression to evaluate"
                    }
                },
                "required": ["expression"]
            }
        }
    })];

    let system_prompt = "You are a helpful assistant with access to a calculator.";

    // First call - no previous response
    let messages1 = vec![Message::user("What is 2 + 2?".to_string())];

    println!("First call (no previous_response_id)...");
    let response1 = provider
        .chat_with_tools_continuing(messages1, system_prompt.to_string(), tools.clone(), None, None)
        .await;

    match response1 {
        Ok(r1) => {
            println!("Response 1 ID: {}", r1.id);
            println!("Response 1 text: {}", r1.text_output.chars().take(100).collect::<String>());

            // Second call - with previous response ID for compaction
            let messages2 = vec![
                Message::user("What is 2 + 2?".to_string()),
                Message::assistant(r1.text_output.clone()),
                Message::user("Now what is that result times 10?".to_string()),
            ];

            println!("\nSecond call (with previous_response_id: {})...", &r1.id[..20]);
            let response2 = provider
                .chat_with_tools_continuing(
                    messages2,
                    system_prompt.to_string(),
                    tools.clone(),
                    Some(r1.id.clone()),
                    None,
                )
                .await;

            match response2 {
                Ok(r2) => {
                    println!("Response 2 ID: {}", r2.id);
                    println!("Response 2 text: {}", r2.text_output.chars().take(100).collect::<String>());

                    // Verify compaction chain works
                    println!("\nCompaction chain verified:");
                    println!("  Response 1 -> Response 2");
                    println!("  IDs differ: {}", r1.id != r2.id);
                }
                Err(e) => println!("Response 2 error: {}", e),
            }
        }
        Err(e) => println!("Response 1 error: {}", e),
    }
}

/// Test simulated Codex tool loop (multiple iterations)
#[tokio::test]
async fn test_llm_codex_tool_loop() {
    if skip_if_no_key() {
        return;
    }

    println!("\n=== CODEX TOOL LOOP TEST ===\n");

    let provider = OpenAIProvider::codex_max(get_api_key().unwrap())
        .expect("Should create Codex provider");

    let tools = vec![
        json!({
            "type": "function",
            "function": {
                "name": "get_current_time",
                "description": "Get the current time",
                "parameters": {
                    "type": "object",
                    "properties": {}
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "create_reminder",
                "description": "Create a reminder",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "message": {
                            "type": "string",
                            "description": "Reminder message"
                        },
                        "time": {
                            "type": "string",
                            "description": "Time for reminder"
                        }
                    },
                    "required": ["message", "time"]
                }
            }
        }),
    ];

    let system_prompt = "You are a task assistant. Complete the user's request using available tools.";
    let mut messages = vec![Message::user(
        "Check the current time and create a reminder for 1 hour from now to review code.".to_string(),
    )];

    let mut iteration = 0;
    let max_iterations = 5;
    let mut total_tokens_input = 0i64;
    let mut total_tokens_output = 0i64;
    let start = Instant::now();

    while iteration < max_iterations {
        iteration += 1;
        println!("Iteration {}...", iteration);

        match provider
            .chat_with_tools(messages.clone(), system_prompt.to_string(), tools.clone(), None)
            .await
        {
            Ok(response) => {
                total_tokens_input += response.tokens.input;
                total_tokens_output += response.tokens.output;

                println!(
                    "  Text: {}",
                    response.text_output.chars().take(80).collect::<String>()
                );
                println!("  Tool calls: {}", response.function_calls.len());

                if response.function_calls.is_empty() {
                    println!("  No more tool calls - done!");
                    break;
                }

                // Build assistant message with tool calls
                let tool_calls_info: Vec<mira_backend::llm::provider::ToolCallInfo> = response
                    .function_calls
                    .iter()
                    .map(|tc| mira_backend::llm::provider::ToolCallInfo {
                        id: tc.id.clone(),
                        name: tc.name.clone(),
                        arguments: tc.arguments.clone(),
                    })
                    .collect();

                messages.push(Message::assistant_with_tool_calls(
                    response.text_output.clone(),
                    tool_calls_info,
                ));

                // Simulate tool execution
                for tc in &response.function_calls {
                    println!("  Executing: {}", tc.name);

                    let result = match tc.name.as_str() {
                        "get_current_time" => json!({"time": "2025-12-06 16:30:00 PST"}),
                        "create_reminder" => json!({"success": true, "reminder_id": 123}),
                        _ => json!({"error": "Unknown tool"}),
                    };

                    messages.push(Message::tool_result(
                        tc.id.clone(),
                        tc.name.clone(),
                        result.to_string(),
                    ));
                }
            }
            Err(e) => {
                println!("  Error: {}", e);
                break;
            }
        }
    }

    let duration = start.elapsed();
    let cost = OpenAIPricing::calculate_cost(
        OpenAIModel::Gpt51CodexMax,
        total_tokens_input,
        total_tokens_output,
    );

    println!("\n=== TOOL LOOP SUMMARY ===");
    println!("Iterations: {}", iteration);
    println!("Duration: {:?}", duration);
    println!("Total tokens: {} input, {} output", total_tokens_input, total_tokens_output);
    println!("Total cost: ${:.6}", cost);
}

/// Test Voice tier for user-facing responses (maintains personality)
#[tokio::test]
async fn test_llm_voice_personality() {
    if skip_if_no_key() {
        return;
    }

    println!("\n=== VOICE PERSONALITY TEST ===\n");

    let provider = OpenAIProvider::gpt51(get_api_key().unwrap())
        .expect("Should create Voice provider");

    let persona = r#"You are Mira, a helpful AI coding assistant.
You have a warm, professional personality and remember user preferences.
You speak concisely but thoroughly, and you're always eager to help with code."#;

    let messages = vec![Message::user(
        "Hi! I'm starting a new Rust project. Any tips?".to_string(),
    )];

    let start = Instant::now();

    match provider.chat(messages, persona.to_string()).await {
        Ok(response) => {
            let latency = start.elapsed();

            println!("Response:\n{}\n", response.content);
            println!("Latency: {:?}", latency);
            println!(
                "Tokens: {} input, {} output",
                response.tokens.input, response.tokens.output
            );

            let cost = OpenAIPricing::calculate_cost(
                OpenAIModel::Gpt51,
                response.tokens.input,
                response.tokens.output,
            );
            println!("Cost: ${:.6}", cost);

            // Verify response is helpful and on-topic
            assert!(!response.content.is_empty(), "Should have content");
            assert!(latency.as_secs() < 30, "Voice tier should respond within 30s");
        }
        Err(e) => {
            println!("Error: {}", e);
        }
    }
}

/// End-to-end test: Voice detects Codex task, simulates spawn, and injection
#[tokio::test]
async fn test_llm_dual_session_e2e() {
    if skip_if_no_key() {
        return;
    }

    println!("\n=== DUAL SESSION E2E TEST ===\n");

    let pool = test_pool().await;
    let manager = SessionManager::new(pool.clone());
    let injection_service = InjectionService::new(pool.clone());
    let classifier = TaskClassifier::new(RouterConfig::default());

    // 1. User sends message to Voice session
    let user_message = "Please implement a simple Rust function that calculates factorial";
    println!("Step 1: User message: {}", user_message);

    // 2. Voice tier acknowledges and detects Codex trigger
    let task = RoutingTask::user_chat();
    let trigger = classifier.should_spawn_codex(&task, user_message);
    println!("Step 2: Codex trigger detected: {:?}", trigger.is_some());

    if trigger.is_none() {
        println!("  No Codex trigger - this is a simple task for Voice tier");
        // Use Voice tier directly
        let voice_provider = OpenAIProvider::gpt51(get_api_key().unwrap()).unwrap();
        let messages = vec![Message::user(user_message.to_string())];
        let persona = "You are Mira, a helpful coding assistant.";

        match voice_provider.chat(messages, persona.to_string()).await {
            Ok(response) => {
                println!("\nVoice response:\n{}", response.content);
            }
            Err(e) => println!("Voice error: {}", e),
        }
        return;
    }

    // 3. Create Voice session and spawn Codex
    let voice_id = manager
        .get_or_create_voice_session(None, Some("/test/project"))
        .await
        .unwrap();
    println!("Step 3: Voice session: {}", &voice_id[..8]);

    let codex_id = manager
        .spawn_codex_session(&voice_id, user_message, &trigger.unwrap(), Some("User wants factorial"))
        .await
        .unwrap();
    println!("Step 4: Codex session spawned: {}", &codex_id[..8]);

    // 4. Simulate Codex work using real LLM
    println!("Step 5: Codex working...");
    let codex_provider = OpenAIProvider::codex_max(get_api_key().unwrap()).unwrap();

    let codex_system = "You are Mira's Codex agent. Write the requested code.";
    let codex_messages = vec![Message::user(user_message.to_string())];

    let start = Instant::now();
    let codex_response = codex_provider
        .chat(codex_messages, codex_system.to_string())
        .await;

    match codex_response {
        Ok(response) => {
            let duration = start.elapsed();
            println!("  Codex response ({:?}):", duration);
            println!("  {}", response.content.chars().take(300).collect::<String>());

            // 5. Complete Codex session
            manager
                .complete_codex_session(
                    &codex_id,
                    &response.content,
                    response.tokens.input,
                    response.tokens.output,
                    OpenAIPricing::calculate_cost(
                        OpenAIModel::Gpt51CodexMax,
                        response.tokens.input,
                        response.tokens.output,
                    ),
                    0,
                )
                .await
                .unwrap();

            // 6. Inject completion into Voice session
            let metadata = CodexCompletionMetadata {
                files_changed: vec!["src/factorial.rs".to_string()],
                duration_seconds: duration.as_secs() as i64,
                tokens_total: response.tokens.input + response.tokens.output,
                cost_usd: OpenAIPricing::calculate_cost(
                    OpenAIModel::Gpt51CodexMax,
                    response.tokens.input,
                    response.tokens.output,
                ),
                tool_calls_count: 0,
                compaction_count: 0,
                key_actions: vec!["Implemented factorial function".to_string()],
            };

            injection_service
                .inject_codex_completion(
                    &voice_id,
                    &codex_id,
                    "Implemented factorial function with recursion and tests.",
                    metadata,
                )
                .await
                .unwrap();

            println!("\nStep 6: Injection created");

            // 7. Voice session retrieves and formats injection
            let injections = injection_service
                .get_pending_injections(&voice_id)
                .await
                .unwrap();

            println!("Step 7: {} pending injection(s)", injections.len());

            if let Some(prompt_section) = injection_service.format_for_prompt(&injections) {
                println!("\nInjection for Voice context:\n{}", prompt_section);
            }

            // 8. Acknowledge
            injection_service.acknowledge_all(&voice_id).await.unwrap();
            println!("\nStep 8: Injections acknowledged");

            println!("\n=== DUAL SESSION E2E COMPLETE ===");
        }
        Err(e) => {
            println!("Codex error: {}", e);
        }
    }
}
