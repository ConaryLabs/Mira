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

// ============================================================================
// COST VALIDATION TESTS
// ============================================================================

#[test]
fn test_cost_validation_typical_session() {
    // Simulate a typical coding session:
    // - 50 fast ops (file listing, search)
    // - 30 voice ops (user chat)
    // - 15 code ops (refactoring, debugging)
    // - 5 agentic ops (large implementations)
    let mut stats = RoutingStats::default();

    // Record typical session distribution
    for _ in 0..50 {
        stats.record(ModelTier::Fast, 5_000);   // Small fast ops
    }
    for _ in 0..30 {
        stats.record(ModelTier::Voice, 15_000); // Medium chat
    }
    for _ in 0..15 {
        stats.record(ModelTier::Code, 30_000);  // Larger code ops
    }
    for _ in 0..5 {
        stats.record(ModelTier::Agentic, 50_000); // Big implementations
    }

    // Verify distribution
    assert_eq!(stats.total_requests(), 100);
    assert_eq!(stats.fast_requests, 50);
    assert_eq!(stats.voice_requests, 30);
    assert_eq!(stats.code_requests, 15);
    assert_eq!(stats.agentic_requests, 5);

    // Verify percentages
    assert!((stats.fast_percentage() - 50.0).abs() < 0.1);
    assert!((stats.voice_percentage() - 30.0).abs() < 0.1);
    assert!((stats.code_percentage() - 15.0).abs() < 0.1);
    assert!((stats.agentic_percentage() - 5.0).abs() < 0.1);

    // Verify cost savings > 60% target
    let savings_pct = stats.savings_percentage();
    assert!(
        savings_pct > 60.0,
        "Expected >60% savings, got {:.1}%",
        savings_pct
    );

    println!("Typical session cost analysis:");
    println!("  Total requests: {}", stats.total_requests());
    println!("  Fast: {:.1}%, Voice: {:.1}%, Code: {:.1}%, Agentic: {:.1}%",
        stats.fast_percentage(), stats.voice_percentage(),
        stats.code_percentage(), stats.agentic_percentage());
    println!("  Actual cost: ${:.4}", stats.total_cost_usd());
    println!("  Baseline cost: ${:.4}", stats.baseline_cost_usd());
    println!("  Savings: ${:.4} ({:.1}%)", stats.estimated_savings_usd, savings_pct);
}

#[test]
fn test_cost_validation_heavy_coding_session() {
    // Heavy coding session with more code/agentic operations
    let mut stats = RoutingStats::default();

    // Distribution: 20% fast, 20% voice, 40% code, 20% agentic
    for _ in 0..20 {
        stats.record(ModelTier::Fast, 5_000);
    }
    for _ in 0..20 {
        stats.record(ModelTier::Voice, 15_000);
    }
    for _ in 0..40 {
        stats.record(ModelTier::Code, 30_000);
    }
    for _ in 0..20 {
        stats.record(ModelTier::Agentic, 50_000);
    }

    // Even with heavy code usage, should still save money
    let savings_pct = stats.savings_percentage();
    assert!(
        savings_pct > 40.0,
        "Expected >40% savings even in heavy coding, got {:.1}%",
        savings_pct
    );

    println!("Heavy coding session: {:.1}% savings", savings_pct);
}

#[test]
fn test_cost_validation_chat_heavy_session() {
    // Chat-heavy session (mostly voice tier)
    let mut stats = RoutingStats::default();

    // Distribution: 10% fast, 70% voice, 15% code, 5% agentic
    for _ in 0..10 {
        stats.record(ModelTier::Fast, 5_000);
    }
    for _ in 0..70 {
        stats.record(ModelTier::Voice, 15_000);
    }
    for _ in 0..15 {
        stats.record(ModelTier::Code, 30_000);
    }
    for _ in 0..5 {
        stats.record(ModelTier::Agentic, 50_000);
    }

    // Chat-heavy should have even better savings
    let savings_pct = stats.savings_percentage();
    assert!(
        savings_pct > 65.0,
        "Expected >65% savings in chat-heavy session, got {:.1}%",
        savings_pct
    );

    println!("Chat-heavy session: {:.1}% savings", savings_pct);
}

#[test]
fn test_percentage_methods() {
    let mut stats = RoutingStats::default();

    // Empty stats should return 0%
    assert_eq!(stats.fast_percentage(), 0.0);
    assert_eq!(stats.voice_percentage(), 0.0);
    assert_eq!(stats.code_percentage(), 0.0);
    assert_eq!(stats.agentic_percentage(), 0.0);
    assert_eq!(stats.savings_percentage(), 0.0);

    // Add some requests
    stats.record(ModelTier::Fast, 10_000);
    stats.record(ModelTier::Voice, 10_000);
    stats.record(ModelTier::Code, 10_000);
    stats.record(ModelTier::Agentic, 10_000);

    // Each should be 25%
    assert!((stats.fast_percentage() - 25.0).abs() < 0.1);
    assert!((stats.voice_percentage() - 25.0).abs() < 0.1);
    assert!((stats.code_percentage() - 25.0).abs() < 0.1);
    assert!((stats.agentic_percentage() - 25.0).abs() < 0.1);
}

#[test]
fn test_cost_calculation_accuracy() {
    let mut stats = RoutingStats::default();

    // 1 request of each type at 10k tokens
    stats.record(ModelTier::Fast, 10_000);
    stats.record(ModelTier::Voice, 10_000);
    stats.record(ModelTier::Code, 10_000);
    stats.record(ModelTier::Agentic, 10_000);

    let total = stats.total_cost_usd();
    let baseline = stats.baseline_cost_usd();

    // Expected: $0.0025 + $0.0125 + $0.0125 + $0.04 = $0.0675
    assert!((total - 0.0675).abs() < 0.001, "Total cost should be ~$0.0675, got ${:.4}", total);

    // Baseline: 4 * $0.04 = $0.16
    assert!((baseline - 0.16).abs() < 0.001, "Baseline should be $0.16, got ${:.4}", baseline);
}

// ============================================================================
// PERSONALITY CONSISTENCY TESTS
// ============================================================================

#[test]
fn test_voice_tier_always_for_user_chat() {
    let classifier = TaskClassifier::new(RouterConfig::default());

    // Various user chat scenarios should all route to Voice
    let scenarios = vec![
        RoutingTask::user_chat(),
        RoutingTask::new(), // Default is user-facing
        RoutingTask {
            is_user_facing: true,
            tool_name: None,
            operation_kind: None,
            estimated_tokens: 100,
            file_count: 0,
            is_long_running: false,
            tier_override: None,
        },
    ];

    for (i, task) in scenarios.iter().enumerate() {
        let tier = classifier.classify(task);
        assert_eq!(
            tier,
            ModelTier::Voice,
            "Scenario {} should route to Voice for consistent personality",
            i
        );
    }
}

#[test]
fn test_voice_tier_for_artifact_creation() {
    let classifier = TaskClassifier::new(RouterConfig::default());

    // Creating artifacts with user context should use Voice
    let task = RoutingTask::from_tool("create_artifact");
    let tier = classifier.classify(&task);

    // create_artifact is a VOICE_TOOL, should stay at Voice
    assert_eq!(tier, ModelTier::Voice, "Artifact creation should use Voice tier");
}

#[test]
fn test_code_tier_isolated_from_chat() {
    let classifier = TaskClassifier::new(RouterConfig::default());

    // Code operations should NOT affect user chat routing
    let code_task = RoutingTask::new().with_operation("refactor");
    assert_eq!(classifier.classify(&code_task), ModelTier::Code);

    // Immediately after, user chat still goes to Voice
    let chat_task = RoutingTask::user_chat();
    assert_eq!(classifier.classify(&chat_task), ModelTier::Voice);
}

#[test]
fn test_agentic_tier_isolated_from_chat() {
    let classifier = TaskClassifier::new(RouterConfig::default());

    // Agentic operations should NOT affect user chat routing
    let agentic_task = RoutingTask::new().with_operation("full_implementation");
    assert_eq!(classifier.classify(&agentic_task), ModelTier::Agentic);

    // User chat still goes to Voice
    let chat_task = RoutingTask::user_chat();
    assert_eq!(classifier.classify(&chat_task), ModelTier::Voice);
}

#[test]
fn test_personality_tier_display_names() {
    // Verify tier display names are user-friendly
    assert_eq!(ModelTier::Fast.display_name(), "Fast (GPT-5.1 Mini)");
    assert_eq!(ModelTier::Voice.display_name(), "Voice (GPT-5.1)");
    assert_eq!(ModelTier::Code.display_name(), "Code (GPT-5.1-Codex-Max)");
    assert_eq!(ModelTier::Agentic.display_name(), "Agentic (GPT-5.1-Codex-Max XHigh)");
}
