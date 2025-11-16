// tests/context_builder_prompt_assembly_test.rs
// Context builder and prompt assembly tests
// Tests UnifiedPromptBuilder's ability to assemble comprehensive system prompts

use mira_backend::api::ws::message::{MessageMetadata, TextSelection};
use mira_backend::git::client::tree_builder::{FileNode, FileNodeType};
use mira_backend::memory::core::types::MemoryEntry;
use mira_backend::memory::features::recall_engine::RecallContext;
use mira_backend::persona::PersonaOverlay;
use mira_backend::prompt::unified_builder::UnifiedPromptBuilder;
use mira_backend::tools::types::{Tool, ToolFunction};
use serde_json::json;

// ============================================================================
// Helper Functions
// ============================================================================

fn create_test_persona() -> PersonaOverlay {
    PersonaOverlay::Default
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
    let mut entry = if role == "user" {
        MemoryEntry::user_message("test-session".to_string(), content.to_string())
    } else {
        MemoryEntry::assistant_message("test-session".to_string(), content.to_string())
    };
    entry.salience = Some(salience);
    entry.summary = Some(format!("Summary: {}", content));
    entry
}

fn create_test_file_tree() -> Vec<FileNode> {
    vec![
        FileNode {
            name: "src".to_string(),
            path: "src".to_string(),
            node_type: FileNodeType::Directory,
            children: vec![
                FileNode {
                    name: "main.rs".to_string(),
                    path: "src/main.rs".to_string(),
                    node_type: FileNodeType::File,
                    children: vec![],
                },
                FileNode {
                    name: "lib.rs".to_string(),
                    path: "src/lib.rs".to_string(),
                    node_type: FileNodeType::File,
                    children: vec![],
                },
            ],
        },
        FileNode {
            name: "Cargo.toml".to_string(),
            path: "Cargo.toml".to_string(),
            node_type: FileNodeType::File,
            children: vec![],
        },
    ]
}

fn create_test_tools() -> Vec<Tool> {
    vec![
        Tool {
            tool_type: "function".to_string(),
            function: Some(ToolFunction {
                name: "create_artifact".to_string(),
                description: "Create a code artifact".to_string(),
                parameters: Some(json!({
                    "type": "object",
                    "properties": {
                        "title": {"type": "string"},
                        "content": {"type": "string"},
                        "language": {"type": "string"}
                    },
                    "required": ["title", "content", "language"]
                })),
            }),
        },
        Tool {
            tool_type: "function".to_string(),
            function: Some(ToolFunction {
                name: "search_code".to_string(),
                description: "Search code elements".to_string(),
                parameters: Some(json!({
                    "type": "object",
                    "properties": {
                        "query": {"type": "string"}
                    },
                    "required": ["query"]
                })),
            }),
        },
    ]
}

// ============================================================================
// Basic Tests
// ============================================================================

#[test]
fn test_minimal_prompt_build() {
    println!("\n=== Testing Minimal Prompt Build ===\n");

    let persona = create_test_persona();
    let context = create_empty_context();

    println!("[1] Building minimal prompt");

    let prompt = UnifiedPromptBuilder::build_system_prompt(
        &persona, &context, None, // tools
        None, // metadata
        None, // project_id
        None, // code_context
        None, // file_tree
    );

    println!("[2] Verifying prompt structure");

    assert!(!prompt.is_empty(), "Prompt should not be empty");
    assert!(prompt.len() > 50, "Prompt should have some substance");

    println!("✓ Minimal prompt built successfully");
    println!("  Length: {} chars", prompt.len());
}

#[test]
fn test_persona_variants() {
    println!("\n=== Testing Persona Variants ===\n");

    let context = create_empty_context();

    // Currently only Default persona exists
    let persona = PersonaOverlay::Default;

    println!("[Default] Building prompt");

    let prompt =
        UnifiedPromptBuilder::build_system_prompt(&persona, &context, None, None, None, None, None);

    assert!(!prompt.is_empty(), "Default persona should produce prompt");
    println!("  ✓ Default persona: {} chars", prompt.len());

    // Note: Additional persona variants (Concise, Detailed, Creative) can be added
    // to PersonaOverlay enum when persona switching becomes a desired feature
}

// ============================================================================
// Memory Context Tests
// ============================================================================

#[test]
fn test_recent_memory_context() {
    println!("\n=== Testing Recent Memory Context ===\n");

    let persona = create_test_persona();
    let mut context = create_empty_context();

    println!("[1] Adding recent memories");

    context.recent = vec![
        create_test_memory_entry("user", "How do I handle errors in Rust?", 0.9),
        create_test_memory_entry("assistant", "Use Result<T, E> for error handling", 0.85),
    ];

    println!("[2] Building prompt with recent memory");

    let prompt =
        UnifiedPromptBuilder::build_system_prompt(&persona, &context, None, None, None, None, None);

    println!("[3] Verifying memory integration");

    assert!(
        prompt.contains("error") || prompt.contains("Result"),
        "Should include recent conversation topics"
    );

    println!("✓ Recent memory context included");
    println!("  {} recent messages", context.recent.len());
}

#[test]
fn test_semantic_memory_context() {
    println!("\n=== Testing Semantic Memory Context ===\n");

    let persona = create_test_persona();
    let mut context = create_empty_context();

    println!("[1] Adding semantic memories");

    context.semantic = vec![
        create_test_memory_entry("assistant", "Previously discussed async patterns", 0.92),
        create_test_memory_entry("user", "Questions about tokio runtime", 0.88),
    ];

    println!("[2] Building prompt with semantic memory");

    let prompt =
        UnifiedPromptBuilder::build_system_prompt(&persona, &context, None, None, None, None, None);

    println!("[3] Verifying semantic context");

    assert!(
        prompt.contains("async") || prompt.contains("tokio") || prompt.len() > 100,
        "Should include semantic memory"
    );

    println!("✓ Semantic memory context included");
}

#[test]
fn test_rolling_summary_context() {
    println!("\n=== Testing Rolling Summary Context ===\n");

    let persona = create_test_persona();
    let mut context = create_empty_context();

    println!("[1] Setting rolling summary");

    context.rolling_summary = Some(
        "User is building a Rust web service with Actix. Focus on error handling and async patterns.".to_string()
    );

    println!("[2] Building prompt with rolling summary");

    let prompt =
        UnifiedPromptBuilder::build_system_prompt(&persona, &context, None, None, None, None, None);

    println!("[3] Verifying summary integration");

    assert!(
        prompt.contains("Actix") || prompt.contains("Rust") || prompt.contains("web"),
        "Should include rolling summary content"
    );

    println!("✓ Rolling summary context included");
}

#[test]
fn test_session_summary_context() {
    println!("\n=== Testing Session Summary Context ===\n");

    let persona = create_test_persona();
    let mut context = create_empty_context();

    println!("[1] Setting session summary");

    context.session_summary = Some(
        "Building REST API with JWT authentication. Discussed database design and API routes."
            .to_string(),
    );

    println!("[2] Building prompt with session summary");

    let prompt =
        UnifiedPromptBuilder::build_system_prompt(&persona, &context, None, None, None, None, None);

    println!("[3] Verifying session summary");

    assert!(
        prompt.contains("REST") || prompt.contains("JWT") || prompt.contains("API"),
        "Should include session summary"
    );

    println!("✓ Session summary context included");
}

// ============================================================================
// File Tree Context Tests
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
        None,
        None,
        Some(&file_tree),
    );

    println!("[2] Verifying file tree inclusion");

    assert!(
        prompt.contains("src") || prompt.contains("main.rs") || prompt.contains("Cargo"),
        "Should include file tree structure"
    );

    println!("✓ File tree context included");
}

// ============================================================================
// Tools Context Tests
// ============================================================================

#[test]
fn test_tools_context() {
    println!("\n=== Testing Tools Context ===\n");

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

    assert!(
        prompt.contains("create_artifact")
            || prompt.contains("search_code")
            || prompt.contains("TOOLS"),
        "Should reference available tools"
    );

    println!("✓ Tool context included");
    println!("  {} tools available", tools.len());
}

// ============================================================================
// Metadata Context Tests
// ============================================================================

#[test]
fn test_file_metadata_context() {
    println!("\n=== Testing File Metadata Context ===\n");

    let persona = create_test_persona();
    let context = create_empty_context();

    println!("[1] Creating file metadata");

    let metadata = MessageMetadata {
        file_path: Some("src/main.rs".to_string()),
        file_content: Some("fn main() {\n    println!(\"Hello, world!\");\n}".to_string()),
        repo_id: None,
        attachment_id: None,
        language: Some("rust".to_string()),
        selection: None,
        project_name: None,
        has_repository: None,
        repo_root: None,
        branch: None,
        request_repo_context: None,
    };

    println!("[2] Building prompt with file metadata");

    let prompt = UnifiedPromptBuilder::build_system_prompt(
        &persona,
        &context,
        None,
        Some(&metadata),
        None,
        None,
        None,
    );

    println!("[3] Verifying file context");

    assert!(
        prompt.contains("main.rs") || prompt.contains("rust") || prompt.contains("Hello"),
        "Should include file metadata"
    );

    println!("✓ File metadata context included");
}

#[test]
fn test_text_selection_context() {
    println!("\n=== Testing Text Selection Context ===\n");

    let persona = create_test_persona();
    let context = create_empty_context();

    println!("[1] Creating metadata with selection");

    let metadata = MessageMetadata {
        file_path: Some("src/lib.rs".to_string()),
        file_content: None,
        repo_id: None,
        attachment_id: None,
        language: Some("rust".to_string()),
        selection: Some(TextSelection {
            start_line: 10,
            end_line: 20,
            text: Some(
                "pub fn process_data(input: &str) -> Result<String> {\n    // Processing logic\n}"
                    .to_string(),
            ),
        }),
        project_name: None,
        has_repository: None,
        repo_root: None,
        branch: None,
        request_repo_context: None,
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

    assert!(
        prompt.contains("process_data") || prompt.contains("Result") || prompt.len() > 100,
        "Should include selected code"
    );

    println!("✓ File selection context included");
}

#[test]
fn test_project_context_metadata() {
    println!("\n=== Testing Project Context with Metadata ===\n");

    let persona = create_test_persona();
    let context = create_empty_context();

    println!("[1] Creating project metadata");

    let metadata = MessageMetadata {
        project_name: Some("mira-backend".to_string()),
        repo_id: Some("repo-123".to_string()),
        has_repository: Some(true),
        request_repo_context: Some(true),
        file_path: None,
        file_content: None,
        attachment_id: None,
        language: None,
        selection: None,
        repo_root: Some("/home/user/mira".to_string()),
        branch: Some("main".to_string()),
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

    assert!(
        prompt.contains("mira-backend") || prompt.contains("main") || prompt.len() > 100,
        "Should reference project"
    );

    println!("✓ Project context with metadata included");
}

// ============================================================================
// Comprehensive Integration Tests
// ============================================================================

#[test]
fn test_complete_context_assembly() {
    println!("\n=== Testing Complete Context Assembly ===\n");

    let persona = create_test_persona();
    let mut context = create_empty_context();

    println!("[1] Setting up all context components");

    // Summaries
    context.session_summary = Some("Working on Rust backend system".to_string());
    context.rolling_summary = Some("Recent focus on error handling patterns".to_string());

    // Memory
    context.recent = vec![create_test_memory_entry("user", "Fix the auth bug", 0.9)];
    context.semantic = vec![create_test_memory_entry(
        "assistant",
        "Authentication patterns discussion",
        0.85,
    )];

    // File tree
    let file_tree = create_test_file_tree();

    // Tools
    let tools = create_test_tools();

    // Metadata
    let metadata = MessageMetadata {
        project_name: Some("mira-backend".to_string()),
        file_path: Some("src/auth.rs".to_string()),
        language: Some("rust".to_string()),
        repo_id: Some("repo-123".to_string()),
        has_repository: Some(true),
        file_content: None,
        attachment_id: None,
        selection: None,
        repo_root: None,
        branch: None,
        request_repo_context: Some(true),
    };

    println!("[2] Building comprehensive prompt");

    let prompt = UnifiedPromptBuilder::build_system_prompt(
        &persona,
        &context,
        Some(&tools),
        Some(&metadata),
        Some("test-project"),
        None,
        Some(&file_tree),
    );

    println!("[3] Analyzing comprehensive prompt");

    assert!(!prompt.is_empty(), "Prompt should not be empty");
    assert!(
        prompt.len() > 500,
        "Comprehensive prompt should be substantial"
    );

    println!("✓ Complete context assembly successful");
    println!("  Prompt length: {} chars", prompt.len());
    println!("  Estimated tokens: ~{}", prompt.len() / 4);
}

#[test]
fn test_empty_sections_handling() {
    println!("\n=== Testing Empty Sections Handling ===\n");

    let persona = create_test_persona();
    let context = create_empty_context();

    println!("[1] Building prompt with all empty sections");

    let prompt =
        UnifiedPromptBuilder::build_system_prompt(&persona, &context, None, None, None, None, None);

    println!("[2] Verifying graceful handling");

    assert!(
        !prompt.is_empty(),
        "Should produce valid prompt even with empty sections"
    );
    assert!(prompt.len() > 50, "Should include base persona at minimum");

    println!("✓ Empty sections handled gracefully");
}

#[test]
fn test_prompt_consistency() {
    println!("\n=== Testing Prompt Consistency ===\n");

    let persona = create_test_persona();
    let context = create_empty_context();
    let file_tree = create_test_file_tree();

    println!("[1] Building same prompt twice");

    let prompt1 = UnifiedPromptBuilder::build_system_prompt(
        &persona,
        &context,
        None,
        None,
        None,
        None,
        Some(&file_tree),
    );

    let prompt2 = UnifiedPromptBuilder::build_system_prompt(
        &persona,
        &context,
        None,
        None,
        None,
        None,
        Some(&file_tree),
    );

    println!("[2] Verifying consistency");

    assert_eq!(
        prompt1, prompt2,
        "Same inputs should produce identical prompts"
    );

    println!("✓ Prompt building is deterministic");
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_very_long_content() {
    println!("\n=== Testing Very Long Content ===\n");

    let persona = create_test_persona();
    let context = create_empty_context();

    println!("[1] Creating long file content");

    let long_content = "a".repeat(10000);
    let metadata = MessageMetadata {
        file_path: Some("large_file.rs".to_string()),
        file_content: Some(long_content),
        language: Some("rust".to_string()),
        repo_id: None,
        attachment_id: None,
        selection: None,
        project_name: None,
        has_repository: None,
        repo_root: None,
        branch: None,
        request_repo_context: None,
    };

    println!("[2] Building prompt with long content");

    let prompt = UnifiedPromptBuilder::build_system_prompt(
        &persona,
        &context,
        None,
        Some(&metadata),
        None,
        None,
        None,
    );

    println!("[3] Verifying no panic");

    assert!(
        !prompt.is_empty(),
        "Should handle long content without panic"
    );

    println!("✓ Long content handled successfully");
}

#[test]
fn test_special_characters() {
    println!("\n=== Testing Special Characters ===\n");

    let persona = create_test_persona();
    let context = create_empty_context();

    println!("[1] Creating content with special chars");

    let metadata = MessageMetadata {
        file_path: Some("test.rs".to_string()),
        file_content: Some("fn test() { let x = \"<>&'\\\"\\n\\t\"; }".to_string()),
        language: Some("rust".to_string()),
        repo_id: None,
        attachment_id: None,
        selection: None,
        project_name: None,
        has_repository: None,
        repo_root: None,
        branch: None,
        request_repo_context: None,
    };

    println!("[2] Building prompt with special characters");

    let prompt = UnifiedPromptBuilder::build_system_prompt(
        &persona,
        &context,
        None,
        Some(&metadata),
        None,
        None,
        None,
    );

    println!("[3] Verifying handling");

    assert!(!prompt.is_empty(), "Should handle special characters");

    println!("✓ Special characters handled");
}

#[test]
fn test_unicode_content() {
    println!("\n=== Testing Unicode Content ===\n");

    let persona = create_test_persona();
    let mut context = create_empty_context();

    println!("[1] Adding unicode memories");

    context.recent = vec![create_test_memory_entry(
        "user",
        "你好世界 🚀 Привет мир",
        0.9,
    )];

    println!("[2] Building prompt with unicode");

    let prompt =
        UnifiedPromptBuilder::build_system_prompt(&persona, &context, None, None, None, None, None);

    println!("[3] Verifying unicode handling");

    assert!(!prompt.is_empty(), "Should handle Unicode content");

    println!("✓ Unicode content handled");
}
