// tests/multi_model_e2e_test.rs
// End-to-end tests for multi-model routing system
//
// Tests the complete flow from task classification through provider execution.
// These tests verify the 4-tier routing system works correctly:
// - Fast (GPT-5.1 Mini): File ops, search
// - Voice (GPT-5.1): User chat
// - Code (GPT-5.1-Codex-Max): Code tasks
// - Agentic (GPT-5.1-Codex-Max XHigh): Long-running tasks

use mira_backend::llm::router::{ModelRouter, RouterConfig, TaskClassifier};
use mira_backend::llm::router::types::{ModelTier, RoutingTask, RoutingStats};
use mira_backend::llm::provider::LlmProvider;
use mira_backend::llm::provider::openai::OpenAIProvider;

use std::sync::Arc;

mod common;

// Helper to create mock router (uses same provider for all tiers)
fn create_test_router() -> ModelRouter {
    // Create mock providers (will fail on actual API calls but work for routing tests)
    let mock_provider: Arc<dyn LlmProvider> = Arc::new(
        OpenAIProvider::gpt51("test-key".to_string())
            .expect("Should create mock provider")
    );

    ModelRouter::new(
        mock_provider.clone(), // Fast
        mock_provider.clone(), // Voice
        mock_provider.clone(), // Code
        mock_provider.clone(), // Agentic
        RouterConfig::default(),
    )
}

#[test]
fn test_user_chat_routes_to_voice() {
    let classifier = TaskClassifier::new(RouterConfig::default());

    // User-facing chat with no tool should go to Voice
    let task = RoutingTask::user_chat();

    let tier = classifier.classify(&task);
    assert_eq!(tier, ModelTier::Voice, "User chat should route to Voice tier");
}

#[test]
fn test_file_operations_route_to_fast() {
    let classifier = TaskClassifier::new(RouterConfig::default());

    // File listing should use Fast tier
    let task = RoutingTask::from_tool("list_files");
    assert_eq!(classifier.classify(&task), ModelTier::Fast);

    // Project file listing should use Fast tier
    let task = RoutingTask::from_tool("list_project_files");
    assert_eq!(classifier.classify(&task), ModelTier::Fast);

    // Codebase search should use Fast tier
    let task = RoutingTask::from_tool("search_codebase");
    assert_eq!(classifier.classify(&task), ModelTier::Fast);

    // Grep should use Fast tier
    let task = RoutingTask::from_tool("grep_files");
    assert_eq!(classifier.classify(&task), ModelTier::Fast);
}

#[test]
fn test_code_operations_route_to_code() {
    let classifier = TaskClassifier::new(RouterConfig::default());

    // Architecture operations should use Code tier
    let task = RoutingTask::new().with_operation("architecture_review");
    assert_eq!(classifier.classify(&task), ModelTier::Code);

    // Refactoring should use Code tier
    let task = RoutingTask::new().with_operation("refactor_multi_file");
    assert_eq!(classifier.classify(&task), ModelTier::Code);

    // Complex debugging should use Code tier
    let task = RoutingTask::new().with_operation("debug_complex");
    assert_eq!(classifier.classify(&task), ModelTier::Code);
}

#[test]
fn test_long_running_tasks_route_to_agentic() {
    let classifier = TaskClassifier::new(RouterConfig::default());

    // Explicitly long-running tasks go to Agentic
    let task = RoutingTask {
        is_user_facing: false,
        tool_name: None,
        operation_kind: Some("implement_feature".to_string()),
        estimated_tokens: 1000,
        file_count: 5,
        is_long_running: true,
        tier_override: None,
    };
    assert_eq!(classifier.classify(&task), ModelTier::Agentic);

    // Full implementation operations go to Agentic
    let task = RoutingTask::new().with_operation("full_implementation");
    assert_eq!(classifier.classify(&task), ModelTier::Agentic);

    // Migration operations go to Agentic
    let task = RoutingTask::new().with_operation("migration");
    assert_eq!(classifier.classify(&task), ModelTier::Agentic);
}

#[test]
fn test_large_context_bumps_to_code() {
    let config = RouterConfig::default();
    let classifier = TaskClassifier::new(config.clone());

    // Small context stays at Voice
    let task = RoutingTask {
        is_user_facing: false,
        tool_name: Some("create_artifact".to_string()),
        operation_kind: None,
        estimated_tokens: 1000,
        file_count: 0,
        is_long_running: false,
        tier_override: None,
    };
    assert_eq!(classifier.classify(&task), ModelTier::Voice);

    // Large context goes to Code
    let task = RoutingTask {
        is_user_facing: false,
        tool_name: Some("create_artifact".to_string()),
        operation_kind: None,
        estimated_tokens: config.code_token_threshold + 1000,
        file_count: 0,
        is_long_running: false,
        tier_override: None,
    };
    assert_eq!(classifier.classify(&task), ModelTier::Code);
}

#[test]
fn test_tier_override_respected() {
    let classifier = TaskClassifier::new(RouterConfig::default());

    // Override should take precedence
    let task = RoutingTask::from_tool("list_files").with_tier(ModelTier::Agentic);
    assert_eq!(classifier.classify(&task), ModelTier::Agentic);

    let task = RoutingTask::new().with_operation("refactor").with_tier(ModelTier::Fast);
    assert_eq!(classifier.classify(&task), ModelTier::Fast);
}

#[test]
fn test_routing_stats_tracking() {
    let mut stats = RoutingStats::default();

    // Record various requests
    stats.record(ModelTier::Fast, 1000);
    stats.record(ModelTier::Voice, 2000);
    stats.record(ModelTier::Code, 5000);
    stats.record(ModelTier::Agentic, 10000);

    assert_eq!(stats.fast_requests, 1);
    assert_eq!(stats.voice_requests, 1);
    assert_eq!(stats.code_requests, 1);
    assert_eq!(stats.agentic_requests, 1);
    assert_eq!(stats.total_requests(), 4);
}

#[test]
fn test_router_provider_selection() {
    let router = create_test_router();

    // Verify we get a provider for each tier
    let fast = router.get_provider(ModelTier::Fast);
    let voice = router.get_provider(ModelTier::Voice);
    let code = router.get_provider(ModelTier::Code);
    let agentic = router.get_provider(ModelTier::Agentic);

    // All should return valid providers (Arc<dyn LlmProvider>)
    assert!(Arc::strong_count(&fast) >= 1);
    assert!(Arc::strong_count(&voice) >= 1);
    assert!(Arc::strong_count(&code) >= 1);
    assert!(Arc::strong_count(&agentic) >= 1);
}

#[test]
fn test_classification_reason_tracking() {
    let classifier = TaskClassifier::new(RouterConfig::default());

    // User-facing reason
    let task = RoutingTask::user_chat();
    let reason = classifier.classification_reason(&task);
    assert!(!reason.is_empty(), "Should have a classification reason");

    // Override reason
    let task = RoutingTask::from_tool("test").with_tier(ModelTier::Code);
    let reason = classifier.classification_reason(&task);
    assert!(reason.contains("override"));
}

#[test]
fn test_convenience_provider_methods() {
    let router = create_test_router();

    // Test convenience methods
    let fast = router.fast();
    let voice = router.voice();
    let code = router.code();
    let agentic = router.agentic();

    // All should return valid providers
    assert!(Arc::strong_count(&fast) >= 1);
    assert!(Arc::strong_count(&voice) >= 1);
    assert!(Arc::strong_count(&code) >= 1);
    assert!(Arc::strong_count(&agentic) >= 1);
}

#[test]
fn test_cost_savings_tracking() {
    let mut stats = RoutingStats::default();

    // Fast tier: 1M tokens (huge savings vs Agentic)
    stats.record(ModelTier::Fast, 1_000_000);
    let savings_after_fast = stats.estimated_savings_usd;
    assert!(savings_after_fast > 0.0, "Fast tier should save money vs Agentic");

    // Voice tier: 1M tokens (moderate savings)
    stats.record(ModelTier::Voice, 1_000_000);
    let savings_after_voice = stats.estimated_savings_usd;
    assert!(savings_after_voice > savings_after_fast, "Voice should add more savings");

    // Agentic tier: 1M tokens (no savings)
    let savings_before_agentic = stats.estimated_savings_usd;
    stats.record(ModelTier::Agentic, 1_000_000);
    let savings_after_agentic = stats.estimated_savings_usd;
    // Agentic doesn't add savings (it's the baseline)
    assert_eq!(savings_after_agentic, savings_before_agentic, "Agentic tier adds no savings");
}

#[test]
fn test_router_summary() {
    let router = create_test_router();

    // Get summary of routing activity
    let summary = router.summary();
    assert!(!summary.is_empty());
    assert!(summary.contains("Routing"));
}

#[test]
fn test_multi_file_threshold() {
    let config = RouterConfig::default();
    let classifier = TaskClassifier::new(config.clone());

    // Few files stays at Voice
    let task = RoutingTask::new()
        .with_files(2);
    assert_eq!(classifier.classify(&task), ModelTier::Voice);

    // Many files goes to Code
    let task = RoutingTask::new()
        .with_files(config.code_file_threshold + 1);
    assert_eq!(classifier.classify(&task), ModelTier::Code);
}
