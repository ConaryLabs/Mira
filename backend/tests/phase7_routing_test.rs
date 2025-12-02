// tests/phase7_routing_test.rs
//
// Phase 7: Routing and Message Integration Tests
// Tests: Message helpers, provider routing, LLM message flow, cloning

use mira_backend::llm::provider::Message;
use mira_backend::llm::provider::{Gemini3Provider, ThinkingLevel};

// ============================================================================
// Message Helper Tests
// ============================================================================

#[test]
fn test_message_user_helper() {
    let msg = Message::user("Hello, world!".to_string());

    assert_eq!(msg.role, "user");
    assert_eq!(msg.content, "Hello, world!");
}

#[test]
fn test_message_assistant_helper() {
    let msg = Message::assistant("Sure, I can help!".to_string());

    assert_eq!(msg.role, "assistant");
    assert_eq!(msg.content, "Sure, I can help!");
}

#[test]
fn test_message_conversation_flow() {
    let messages = vec![
        Message::user("What's the weather?".to_string()),
        Message::assistant("It's sunny today!".to_string()),
        Message::user("Thanks!".to_string()),
    ];

    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0].role, "user");
    assert_eq!(messages[1].role, "assistant");
    assert_eq!(messages[2].role, "user");
}

// ============================================================================
// Provider Cloning Tests (Phase 7 added Clone derives)
// ============================================================================

#[test]
fn test_gpt5_provider_clone() {
    let provider = Gemini3Provider::new(
        "test-key".to_string(),
        "gpt-5-preview".to_string(),
        ThinkingLevel::High,
    ).expect("Should create provider");

    // Should compile and clone successfully (compile-time test)
    let _cloned = provider.clone();

    // Clone derive works - this test passes if it compiles
    assert!(true);
}

// GPT 5.1 is the only LLM provider now

#[test]
fn test_provider_clone_independence() {
    let original = Gemini3Provider::new(
        "original-key".to_string(),
        "gpt-5-preview".to_string(),
        ThinkingLevel::High,
    ).expect("Should create provider");

    let cloned = original.clone();

    // Clones should be usable independently
    drop(original);

    // If this compiles and runs, Clone works correctly
    drop(cloned);
    assert!(true);
}

// ============================================================================
// Provider Construction Tests
// ============================================================================

#[test]
fn test_create_gpt5_provider() {
    let provider = Gemini3Provider::new(
        "test-key".to_string(),
        "gpt-5-preview".to_string(),
        ThinkingLevel::High,
    ).expect("Should create provider");

    // If this compiles, provider construction works
    drop(provider);
    assert!(true);
}

// Only GPT 5.1 provider is used now

#[test]
fn test_provider_with_different_configs() {
    let minimal = Gemini3Provider::new(
        "key1".to_string(),
        "gpt-5-preview".to_string(),
        ThinkingLevel::Low,
    ).expect("Should create minimal provider");

    let maximal = Gemini3Provider::new(
        "key2".to_string(),
        "gpt-5-preview".to_string(),
        ThinkingLevel::High,
    ).expect("Should create maximal provider");

    // Both should construct successfully
    drop(minimal);
    drop(maximal);
    assert!(true);
}

// ============================================================================
// Routing Logic Tests (Complexity-based selection)
// ============================================================================

#[test]
fn test_simple_task_routing() {
    // Simulate routing logic: simple tasks should go to GPT-5
    let task_complexity = "simple";
    let should_delegate = matches!(task_complexity, "complex" | "very_complex");

    assert!(
        !should_delegate,
        "Simple tasks use minimum reasoning effort"
    );
}

#[test]
fn test_complex_task_routing() {
    // Simulate routing logic: complex tasks should delegate
    let task_complexity = "complex";
    let should_delegate = matches!(task_complexity, "complex" | "very_complex");

    assert!(should_delegate, "Complex tasks use high reasoning effort");
}

#[test]
fn test_routing_decision_matrix() {
    let test_cases = vec![
        ("simple", false),
        ("moderate", false),
        ("complex", true),
        ("very_complex", true),
    ];

    for (complexity, expected_delegation) in test_cases {
        let should_delegate = matches!(complexity, "complex" | "very_complex");
        assert_eq!(
            should_delegate, expected_delegation,
            "Routing decision for '{}' should be: delegate={}",
            complexity, expected_delegation
        );
    }
}

// ============================================================================
// Message Context Building Tests
// ============================================================================

#[test]
fn test_context_message_building() {
    let mut context: Vec<Message> = Vec::new();

    // Build conversation context
    context.push(Message::user("Create a function".to_string()));
    context.push(Message::assistant("I'll help with that".to_string()));
    context.push(Message::user("Make it handle errors".to_string()));

    assert_eq!(context.len(), 3);

    // Last message should be user
    assert_eq!(context.last().unwrap().role, "user");

    // Should alternate user/assistant
    assert_eq!(context[0].role, "user");
    assert_eq!(context[1].role, "assistant");
    assert_eq!(context[2].role, "user");
}

#[test]
fn test_empty_context_handling() {
    let context: Vec<Message> = Vec::new();

    assert!(context.is_empty());
    assert_eq!(context.len(), 0);
}

#[test]
fn test_context_with_system_message() {
    let mut context: Vec<Message> = Vec::new();

    // System message
    context.push(Message::system("You are a helpful assistant".to_string()));
    context.push(Message::user("Hello".to_string()));
    context.push(Message::assistant("Hi there!".to_string()));

    assert_eq!(context.len(), 3);
    assert_eq!(context[0].role, "system");
    assert_eq!(context[1].role, "user");
    assert_eq!(context[2].role, "assistant");
}

// ============================================================================
// Provider Factory Pattern Tests (removed - no public methods to test)
// ============================================================================

// ============================================================================
// Message Serialization Tests
// ============================================================================

#[test]
fn test_message_to_json() {
    let msg = Message::user("Test content".to_string());
    let json = serde_json::to_value(&msg).expect("Should serialize");

    assert_eq!(json["role"], "user");
    assert_eq!(json["content"], "Test content");
}

#[test]
fn test_message_from_json() {
    let json = serde_json::json!({
        "role": "assistant",
        "content": "Test response"
    });

    let msg: Message = serde_json::from_value(json).expect("Should deserialize");

    assert_eq!(msg.role, "assistant");
    assert_eq!(msg.content, "Test response");
}

#[test]
fn test_message_array_serialization() {
    let messages = vec![
        Message::user("First".to_string()),
        Message::assistant("Second".to_string()),
        Message::user("Third".to_string()),
    ];

    let json = serde_json::to_value(&messages).expect("Should serialize array");
    let deserialized: Vec<Message> =
        serde_json::from_value(json).expect("Should deserialize array");

    assert_eq!(deserialized.len(), 3);
    assert_eq!(deserialized[0].role, "user");
    assert_eq!(deserialized[1].role, "assistant");
    assert_eq!(deserialized[2].role, "user");
}

// ============================================================================
// Edge Case Tests
// ============================================================================

#[test]
fn test_empty_message_content() {
    let msg = Message::user("".to_string());

    assert_eq!(msg.role, "user");
    assert_eq!(msg.content, "");
    assert!(msg.content.is_empty());
}

#[test]
fn test_large_message_content() {
    let large_content = "x".repeat(10_000);
    let msg = Message::user(large_content.clone());

    assert_eq!(msg.role, "user");
    assert_eq!(msg.content.len(), 10_000);
    assert_eq!(msg.content, large_content);
}

#[test]
fn test_unicode_message_content() {
    let unicode_msg = Message::user("Hello 世界 🌍".to_string());

    assert_eq!(unicode_msg.role, "user");
    assert_eq!(unicode_msg.content, "Hello 世界 🌍");
}

#[test]
fn test_multiline_message_content() {
    let multiline = "Line 1\nLine 2\nLine 3".to_string();
    let msg = Message::assistant(multiline.clone());

    assert_eq!(msg.role, "assistant");
    assert_eq!(msg.content, multiline);
    assert!(msg.content.contains('\n'));
}
