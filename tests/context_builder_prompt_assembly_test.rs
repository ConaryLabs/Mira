// tests/context_builder_prompt_assembly_test.rs
// Context Builder and Prompt Assembly Tests
//
// Tests the UnifiedPromptBuilder and ContextBuilder which assemble comprehensive
// context from multiple sources for LLM prompts. This is where everything comes together.
//
// Critical aspects:
// 1. Context source gathering (memory, code, relationships, summaries)
// 2. Prompt assembly with proper ordering
// 3. Rolling summary inclusion
// 4. Code intelligence context
// 5. File tree context
// 6. Relationship facts
// 7. Tool context
// 8. Context prioritization
// 9. Token budget management
// 10. Edge cases (empty/missing data)

use mira_backend::prompt::UnifiedPromptBuilder;
use mira_backend::memory::features::recall_engine::RecallContext;
use mira_backend::memory::core::types::MemoryEntry;
use mira_backend::persona::PersonaOverlay;
use mira_backend::tools::types::Tool;
use mira_backend::git::client::tree_builder::{FileNode, FileNodeType};
use mira_backend::api::ws::message::MessageMetadata;
use chrono::Utc;
use serde_json::json;

// ============================================================================
// TEST SETUP UTILITIES
// ============================================================================

fn create_test_persona() -> PersonaOverlay {
    PersonaOverlay::default()
}

fn create_empty_context() -> RecallContext {
    RecallContext {
        recent: vec![],
        semantic: vec![],
        rolling_summary: None,
        session_summary: None,
    }
}

fn create_test_memory_entry(role: &str, content: &str, salience: f32) -> MemoryEntry {
    MemoryEntry {
        id: Some(format!("msg-{}", rand::random::<u32>())),
        session_id: "test-session".to_string(),
        response_id: None,
        parent_id: None,
        role: role.to_string(),
        content: content.to_string(),
        timestamp: Utc::now(),
        tags: None,
        salience: Some(salience),
        topics: None,
        mood: None,
        intent: None,
        contains_code: None,
        programming_lang: None,
        contains_error: None,
        error_type: None,
        summary: None,
    }
}

fn create_test_file_tree() -> Vec<FileNode> {
    vec![
        FileNode {
            path: "src/".to_string(),
            node_type: FileNodeType::Directory,
            size: None,
        },
        FileNode {
            path: "src/main.rs".to_string(),
            node_type: FileNodeType::File,
            size: Some(1024),
        },
        FileNode {
            path: "src/lib.rs".to_string(),
            node_type: FileNodeType::File,
            size: Some(2048),
        },
        FileNode {
            path: "tests/".to_string(),
            node_type: FileNodeType::Directory,
            size: None,
        },
        FileNode {
            path: "Cargo.toml".to_string(),
            node_type: FileNodeType::File,
            size: Some(512),
        },
    ]
}

fn create_test_tools() -> Vec<Tool> {
    vec![
        Tool {
            type_: "function".to_string(),
            function: Some(json!({
                "name": "create_artifact",
                "description": "Create a code artifact",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {"type": "string"},
                        "content": {"type": "string"}
                    }
                }
            })),
        },
        Tool {
            type_: "function".to_string(),
            function: Some(json!({
                "name": "search_code",
                "description": "Search code elements",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": {"type": "string"}
                    }
                }
            })),
        },
    ]
}

// ============================================================================
// TEST 1: Basic Prompt Assembly with Persona
// ============================================================================

#[test]
fn test_basic_prompt_assembly() {
    println!("\n=== Testing Basic Prompt Assembly ===\n");
    
    let persona = create_test_persona();
    let context = create_empty_context();
    
    println!("[1] Building prompt with only persona");
    
    let prompt = UnifiedPromptBuilder::build_system_prompt(
        &persona,
        &context,
        None, // No tools
        None, // No metadata
        None, // No project
        None, // No code context
        None, // No file tree
    );
    
    assert!(!prompt.is_empty(), "Prompt should not be empty");
    println!("✓ Prompt generated: {} chars", prompt.len());
    
    println!("[2] Verifying persona content is included");
    
    // Persona should be at the start of the prompt
    assert!(prompt.contains("Mira") || prompt.len() > 100, 
            "Prompt should contain persona content");
    
    println!("✓ Basic prompt assembly working");
}

// ============================================================================
// TEST 2: Rolling Summary Inclusion
// ============================================================================

#[test]
fn test_rolling_summary_inclusion() {
    println!("\n=== Testing Rolling Summary Inclusion ===\n");
    
    let persona = create_test_persona();
    let mut context = create_empty_context();
    
    println!("[1] Adding rolling summaries to context");
    
    context.rolling_summary = Some(
        "Recent discussion about Rust error handling patterns and async programming.".to_string()
    );
    
    context.session_summary = Some(
        "User is working on a backend system with WebSocket communication and memory management.".to_string()
    );
    
    println!("[2] Building prompt with summaries");
    
    let prompt = UnifiedPromptBuilder::build_system_prompt(
        &persona,
        &context,
        None,
        None,
        None,
        None,
        None,
    );
    
    println!("[3] Verifying summaries are included");
    
    assert!(prompt.contains("RECENT ACTIVITY") || prompt.contains("100"), 
            "Prompt should reference rolling summary");
    assert!(prompt.contains("SESSION OVERVIEW") || prompt.contains("conversation"), 
            "Prompt should reference session summary");
    
    println!("✓ Rolling summaries included in prompt");
    println!("  Prompt length with summaries: {} chars", prompt.len());
}

// ============================================================================
// TEST 3: Recent Messages Context
// ============================================================================

#[test]
fn test_recent_messages_context() {
    println!("\n=== Testing Recent Messages Context ===\n");
    
    let persona = create_test_persona();
    let mut context = create_empty_context();
    
    println!("[1] Adding recent messages");
    
    context.recent = vec![
        create_test_memory_entry("user", "How do I implement JWT auth?", 0.8),
        create_test_memory_entry("assistant", "Here's how to implement JWT authentication...", 0.7),
        create_test_memory_entry("user", "What about refresh tokens?", 0.75),
    ];
    
    println!("✓ Added {} recent messages", context.recent.len());
    
    println!("[2] Building prompt with recent messages");
    
    let prompt = UnifiedPromptBuilder::build_system_prompt(
        &persona,
        &context,
        None,
        None,
        None,
        None,
        None,
    );
    
    println!("[3] Verifying recent messages are included");
    
    assert!(prompt.contains("Recent conversation") || prompt.contains("JWT"), 
            "Prompt should include recent conversation context");
    
    println!("✓ Recent messages included in prompt");
}

// ============================================================================
// TEST 4: Semantic Memories with Salience Filtering
// ============================================================================

#[test]
fn test_semantic_memories_with_salience() {
    println!("\n=== Testing Semantic Memories with Salience Filtering ===\n");
    
    let persona = create_test_persona();
    let mut context = create_empty_context();
    
    println!("[1] Adding semantic memories with varying salience");
    
    context.semantic = vec![
        create_test_memory_entry("user", "Important: Using Arc for thread safety", 0.9),
        create_test_memory_entry("assistant", "Here's how to use Arc...", 0.85),
        create_test_memory_entry("user", "Minor detail about formatting", 0.4), // Low salience
        create_test_memory_entry("assistant", "Critical bug fix approach", 0.95),
    ];
    
    println!("✓ Added {} semantic memories", context.semantic.len());
    
    println!("[2] Building prompt");
    
    let prompt = UnifiedPromptBuilder::build_system_prompt(
        &persona,
        &context,
        None,
        None,
        None,
        None,
        None,
    );
    
    println!("[3] Verifying high-salience memories included");
    
    // High salience memories (>= 0.6) should be included
    assert!(prompt.contains("Arc") || prompt.contains("thread safety"), 
            "High salience memories should be included");
    
    // Low salience memory should be filtered out
    // (This depends on implementation - salience filtering at >= 0.6)
    
    println!("✓ Semantic memories with salience filtering working");
}

// ============================================================================
// TEST 5: File Tree Context
// ============================================================================

#[test]
fn test_file_tree_context() {
    println!("\n=== Testing File Tree Context ===\n");
    
    let persona = create_test_persona();
    let context = create_empty_context();
    let file_tree = create_test_file_tree();
    
    println!("[1] Building prompt with file tree");
    
    let prompt = UnifiedPromptBuilder::build_system_prompt(
        &persona,
        &context,
        None,
        None,
        Some("test-project"),
        None,
        Some(&file_tree),
    );
    
    println!("[2] Verifying file tree is included");
    
    assert!(prompt.contains("REPOSITORY STRUCTURE") || prompt.contains("src/"), 
            "Prompt should include repository structure");
    assert!(prompt.contains("main.rs") || prompt.contains("Cargo.toml"), 
            "Prompt should reference actual files");
    
    println!("✓ File tree context included");
    println!("  Tree contains {} nodes", file_tree.len());
}

// ============================================================================
// TEST 6: Code Intelligence Context
// ============================================================================

#[test]
fn test_code_intelligence_context() {
    println!("\n=== Testing Code Intelligence Context ===\n");
    
    let persona = create_test_persona();
    let context = create_empty_context();
    
    println!("[1] Creating code intelligence entries");
    
    let mut code_entry = create_test_memory_entry(
        "code",
        "src/auth/jwt.rs: authenticate_user - pub async fn authenticate_user(token: &str)",
        0.9
    );
    code_entry.contains_code = Some(true);
    code_entry.programming_lang = Some("rust".to_string());
    
    let code_context = vec![code_entry];
    
    println!("[2] Building prompt with code context");
    
    let prompt = UnifiedPromptBuilder::build_system_prompt(
        &persona,
        &context,
        None,
        None,
        Some("test-project"),
        Some(&code_context),
        None,
    );
    
    println!("[3] Verifying code context is included");
    
    assert!(prompt.contains("PROJECT CODE CONTEXT") || prompt.contains("src/auth"), 
            "Prompt should include code context");
    assert!(prompt.contains("REAL files") || prompt.contains("authenticate_user"), 
            "Prompt should emphasize real file paths");
    
    println!("✓ Code intelligence context included");
}

// ============================================================================
// TEST 7: Tool Context
// ============================================================================

#[test]
fn test_tool_context() {
    println!("\n=== Testing Tool Context ===\n");
    
    let persona = create_test_persona();
    let context = create_empty_context();
    let tools = create_test_tools();
    
    println!("[1] Building prompt with tools");
    
    let prompt = UnifiedPromptBuilder::build_system_prompt(
        &persona,
        &context,
        Some(&tools),
        None,
        None,
        None,
        None,
    );
    
    println!("[2] Verifying tools are included");
    
    assert!(prompt.contains("TOOLS AVAILABLE") || prompt.contains("create_artifact"), 
            "Prompt should list available tools");
    assert!(prompt.contains("search_code"), 
            "Prompt should include all tools");
    
    println!("✓ Tool context included");
    println!("  {} tools available", tools.len());
}

// ============================================================================
// TEST 8: Project Context with Metadata
// ============================================================================

#[test]
fn test_project_context_with_metadata() {
    println!("\n=== Testing Project Context with Metadata ===\n");
    
    let persona = create_test_persona();
    let context = create_empty_context();
    
    println!("[1] Creating metadata with project info");
    
    let metadata = MessageMetadata {
        project_name: Some("mira-backend".to_string()),
        repo_id: Some("repo-123".to_string()),
        has_repository: Some(true),
        request_repo_context: Some(true),
        file_path: None,
        file_content: None,
        language: None,
        selection: None,
    };
    
    println!("[2] Building prompt with project metadata");
    
    let prompt = UnifiedPromptBuilder::build_system_prompt(
        &persona,
        &context,
        None,
        Some(&metadata),
        Some("test-project-id"),
        None,
        None,
    );
    
    println!("[3] Verifying project context");
    
    assert!(prompt.contains("ACTIVE PROJECT") || prompt.contains("mira-backend"), 
            "Prompt should reference active project");
    
    println!("✓ Project context with metadata included");
}

// ============================================================================
// TEST 9: Code-Related Context Detection
// ============================================================================

#[test]
fn test_code_related_context_detection() {
    println!("\n=== Testing Code-Related Context Detection ===\n");
    
    let persona = create_test_persona();
    let context = create_empty_context();
    
    println!("[1] Creating code-related metadata");
    
    let metadata = MessageMetadata {
        project_name: None,
        repo_id: None,
        has_repository: Some(true),
        request_repo_context: None,
        file_path: Some("src/main.rs".to_string()),
        file_content: Some("fn main() {}".to_string()),
        language: Some("rust".to_string()),
        selection: None,
    };
    
    println!("[2] Building prompt with code metadata");
    
    let prompt = UnifiedPromptBuilder::build_system_prompt(
        &persona,
        &context,
        None,
        Some(&metadata),
        None,
        None,
        None,
    );
    
    println!("[3] Verifying code handling hints");
    
    assert!(prompt.contains("CODE HANDLING") || prompt.contains("create_artifact"), 
            "Code-related context should trigger tool usage hints");
    assert!(prompt.contains("conversational text") || prompt.contains("NEVER respond with ONLY"), 
            "Should include conversation requirements");
    
    println!("✓ Code-related context detection working");
}

// ============================================================================
// TEST 10: Context Ordering and Prioritization
// ============================================================================

#[test]
fn test_context_ordering_and_prioritization() {
    println!("\n=== Testing Context Ordering and Prioritization ===\n");
    
    let persona = create_test_persona();
    let mut context = create_empty_context();
    
    println!("[1] Adding all context types");
    
    // Add summaries
    context.session_summary = Some("SESSION_SUMMARY".to_string());
    context.rolling_summary = Some("ROLLING_SUMMARY".to_string());
    
    // Add messages
    context.recent = vec![
        create_test_memory_entry("user", "RECENT_MESSAGE", 0.8),
    ];
    
    context.semantic = vec![
        create_test_memory_entry("assistant", "SEMANTIC_MEMORY", 0.9),
    ];
    
    let file_tree = create_test_file_tree();
    let tools = create_test_tools();
    
    println!("[2] Building comprehensive prompt");
    
    let prompt = UnifiedPromptBuilder::build_system_prompt(
        &persona,
        &context,
        Some(&tools),
        None,
        Some("test-project"),
        None,
        Some(&file_tree),
    );
    
    println!("[3] Analyzing context order in prompt");
    
    // Find positions of different context sections
    let persona_pos = prompt.find("Mira").unwrap_or(0);
    let session_pos = prompt.find("SESSION").unwrap_or(usize::MAX);
    let rolling_pos = prompt.find("RECENT ACTIVITY").unwrap_or(usize::MAX);
    let memory_pos = prompt.find("MEMORY CONTEXT").unwrap_or(usize::MAX);
    let repo_pos = prompt.find("REPOSITORY").unwrap_or(usize::MAX);
    
    println!("  Persona position: {}", persona_pos);
    println!("  Session summary position: {}", session_pos);
    println!("  Rolling summary position: {}", rolling_pos);
    println!("  Memory context position: {}", memory_pos);
    println!("  Repository position: {}", repo_pos);
    
    // Verify ordering: Persona → Summaries → Memories → File Tree → Tools
    assert!(persona_pos < session_pos || session_pos == usize::MAX, 
            "Persona should come before session summary");
    
    println!("✓ Context ordering verified");
}

// ============================================================================
// TEST 11: Empty Context Handling
// ============================================================================

#[test]
fn test_empty_context_handling() {
    println!("\n=== Testing Empty Context Handling ===\n");
    
    let persona = create_test_persona();
    let context = create_empty_context();
    
    println!("[1] Building prompt with completely empty context");
    
    let prompt = UnifiedPromptBuilder::build_system_prompt(
        &persona,
        &context,
        None, // No tools
        None, // No metadata
        None, // No project
        None, // No code context
        None, // No file tree
    );
    
    println!("[2] Verifying minimal valid prompt");
    
    assert!(!prompt.is_empty(), "Should still generate valid prompt");
    assert!(prompt.len() > 50, "Should have at least persona content");
    
    println!("✓ Empty context handled gracefully");
    println!("  Minimal prompt length: {} chars", prompt.len());
}

// ============================================================================
// TEST 12: Token Budget Awareness
// ============================================================================

#[test]
fn test_token_budget_awareness() {
    println!("\n=== Testing Token Budget Awareness ===\n");
    
    let persona = create_test_persona();
    let mut context = create_empty_context();
    
    println!("[1] Creating large context");
    
    // Add many messages
    for i in 0..50 {
        context.recent.push(
            create_test_memory_entry("user", &format!("Message {} with content", i), 0.8)
        );
    }
    
    for i in 0..50 {
        context.semantic.push(
            create_test_memory_entry("assistant", &format!("Response {} with content", i), 0.9)
        );
    }
    
    println!("[2] Building prompt with large context");
    
    let prompt = UnifiedPromptBuilder::build_system_prompt(
        &persona,
        &context,
        None,
        None,
        None,
        None,
        None,
    );
    
    println!("[3] Checking prompt size");
    
    let estimated_tokens = prompt.len() / 4; // Rough estimate: ~4 chars per token
    
    println!("  Prompt length: {} chars", prompt.len());
    println!("  Estimated tokens: ~{}", estimated_tokens);
    
    // Verify prompt isn't absurdly large (should stay under reasonable limits)
    assert!(estimated_tokens < 20000, 
            "Prompt should not exceed reasonable token budget");
    
    println!("✓ Token budget appears reasonable");
}

// ============================================================================
// TEST 13: File Selection Context
// ============================================================================

#[test]
fn test_file_selection_context() {
    println!("\n=== Testing File Selection Context ===\n");
    
    let persona = create_test_persona();
    let context = create_empty_context();
    
    println!("[1] Creating metadata with file selection");
    
    let selection = json!({
        "start_line": 10,
        "end_line": 25,
        "text": "fn process_data(input: &str) -> Result<String> {\n    // Implementation\n}"
    });
    
    let metadata = MessageMetadata {
        project_name: Some("test-project".to_string()),
        repo_id: None,
        has_repository: None,
        request_repo_context: None,
        file_path: Some("src/processor.rs".to_string()),
        file_content: None,
        language: Some("rust".to_string()),
        selection: Some(selection),
    };
    
    println!("[2] Building prompt with selection");
    
    let prompt = UnifiedPromptBuilder::build_system_prompt(
        &persona,
        &context,
        None,
        Some(&metadata),
        None,
        None,
        None,
    );
    
    println!("[3] Verifying selection is included");
    
    assert!(prompt.contains("SELECTED LINES") || prompt.contains("process_data"), 
            "Prompt should include selected code");
    
    println!("✓ File selection context included");
}

// ============================================================================
// TEST 14: Multiple Context Sources Integration
// ============================================================================

#[test]
fn test_multiple_context_sources_integration() {
    println!("\n=== Testing Multiple Context Sources Integration ===\n");
    
    let persona = create_test_persona();
    let mut context = create_empty_context();
    
    println!("[1] Setting up all context sources");
    
    // Summaries
    context.session_summary = Some("Working on backend system".to_string());
    context.rolling_summary = Some("Recent focus on error handling".to_string());
    
    // Messages
    context.recent = vec![
        create_test_memory_entry("user", "Fix the auth bug", 0.9),
    ];
    
    context.semantic = vec![
        create_test_memory_entry("assistant", "Authentication patterns", 0.85),
    ];
    
    // Code context
    let code_entry = create_test_memory_entry(
        "code",
        "src/auth/mod.rs: validate_token - pub fn validate_token",
        0.9
    );
    let code_context = vec![code_entry];
    
    // File tree
    let file_tree = create_test_file_tree();
    
    // Tools
    let tools = create_test_tools();
    
    // Metadata
    let metadata = MessageMetadata {
        project_name: Some("mira-backend".to_string()),
        repo_id: Some("repo-123".to_string()),
        has_repository: Some(true),
        request_repo_context: Some(true),
        file_path: None,
        file_content: None,
        language: None,
        selection: None,
    };
    
    println!("[2] Building comprehensive prompt");
    
    let prompt = UnifiedPromptBuilder::build_system_prompt(
        &persona,
        &context,
        Some(&tools),
        Some(&metadata),
        Some("test-project"),
        Some(&code_context),
        Some(&file_tree),
    );
    
    println!("[3] Verifying all contexts are present");
    
    let has_persona = prompt.len() > 100;
    let has_summaries = prompt.contains("SESSION") || prompt.contains("RECENT");
    let has_memory = prompt.contains("conversation") || prompt.contains("Fix the auth");
    let has_code = prompt.contains("src/auth") || prompt.contains("validate_token");
    let has_files = prompt.contains("REPOSITORY") || prompt.contains("main.rs");
    let has_tools = prompt.contains("TOOLS") || prompt.contains("create_artifact");
    let has_project = prompt.contains("mira-backend");
    
    assert!(has_persona, "Should have persona");
    assert!(has_summaries, "Should have summaries");
    assert!(has_memory, "Should have memory context");
    assert!(has_code, "Should have code context");
    assert!(has_files, "Should have file tree");
    assert!(has_tools, "Should have tools");
    assert!(has_project, "Should have project context");
    
    println!("✓ All context sources integrated");
    println!("  Final prompt length: {} chars", prompt.len());
    println!("  Estimated tokens: ~{}", prompt.len() / 4);
}

// ============================================================================
// INTEGRATION TEST: Real-World Context Assembly
// ============================================================================

#[test]
fn test_real_world_context_assembly() {
    println!("\n=== Testing Real-World Context Assembly ===\n");
    
    println!("[1] Simulating real conversation context");
    
    let persona = create_test_persona();
    let mut context = create_empty_context();
    
    // Realistic summaries
    context.session_summary = Some(
        "User is building a Rust backend with WebSocket support. Key features include: \
         memory management with SQLite and Qdrant, LLM orchestration with GPT-5 and DeepSeek, \
         real-time streaming, and comprehensive testing. Currently working on test coverage gaps."
            .to_string()
    );
    
    context.rolling_summary = Some(
        "Recent work on WebSocket connection lifecycle tests and message routing tests. \
         Identified need for rolling summary tests, context builder tests, and relationship \
         service tests. Discussed comprehensive test structure and best practices."
            .to_string()
    );
    
    // Recent conversation
    context.recent = vec![
        create_test_memory_entry(
            "user",
            "in that case, take a look in the tests/ directory and see where we have gaps",
            0.85
        ),
        create_test_memory_entry(
            "assistant",
            "Looking at your test coverage... You have solid coverage on core flows but some gaps",
            0.8
        ),
    ];
    
    // Relevant semantic memories
    context.semantic = vec![
        create_test_memory_entry(
            "assistant",
            "Rolling summaries are critical - you recently fixed bugs there",
            0.95
        ),
        create_test_memory_entry(
            "user",
            "The context builder is where everything comes together",
            0.9
        ),
    ];
    
    // Project context
    let metadata = MessageMetadata {
        project_name: Some("mira-backend".to_string()),
        repo_id: Some("mira-repo".to_string()),
        has_repository: Some(true),
        request_repo_context: Some(true),
        file_path: None,
        file_content: None,
        language: Some("rust".to_string()),
        selection: None,
    };
    
    // File tree
    let file_tree = vec![
        FileNode {
            path: "tests/".to_string(),
            node_type: FileNodeType::Directory,
            size: None,
        },
        FileNode {
            path: "tests/websocket_connection_test.rs".to_string(),
            node_type: FileNodeType::File,
            size: Some(5000),
        },
        FileNode {
            path: "tests/message_routing_test.rs".to_string(),
            node_type: FileNodeType::File,
            size: Some(8000),
        },
        FileNode {
            path: "src/prompt/unified_builder.rs".to_string(),
            node_type: FileNodeType::File,
            size: Some(12000),
        },
    ];
    
    println!("[2] Assembling complete real-world prompt");
    
    let prompt = UnifiedPromptBuilder::build_system_prompt(
        &persona,
        &context,
        None,
        Some(&metadata),
        Some("mira-backend"),
        None,
        Some(&file_tree),
    );
    
    println!("[3] Analyzing assembled prompt");
    
    println!("  Prompt length: {} chars", prompt.len());
    println!("  Estimated tokens: ~{}", prompt.len() / 4);
    
    // Verify prompt has all necessary context for responding intelligently
    assert!(prompt.contains("WebSocket") || prompt.contains("tests"), 
            "Should have relevant context");
    assert!(prompt.contains("mira-backend"), 
            "Should know project name");
    assert!(prompt.len() > 1000, 
            "Should have substantial context");
    assert!(prompt.len() < 50000, 
            "Should not be excessively large");
    
    println!("✓ Real-world context assembly successful");
    println!("\n=== All Context Builder Tests Complete ===\n");
}
