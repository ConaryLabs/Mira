// tests/recall_engine_oracle_test.rs
// Tests for RecallEngine with Context Oracle integration

use mira_backend::context_oracle::{ContextConfig, ContextOracle, GatheredContext};
use mira_backend::memory::features::recall_engine::{RecallConfig, RecallContext};
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;
use std::sync::Arc;

// ============================================================================
// Test Helpers
// ============================================================================

async fn setup_test_pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect(":memory:")
        .await
        .expect("Failed to create in-memory pool");

    // Create minimal schema for tests
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS memory_entries (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id TEXT NOT NULL,
            response_id TEXT,
            parent_id INTEGER,
            role TEXT NOT NULL,
            content TEXT NOT NULL,
            timestamp TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            tags TEXT,
            mood TEXT,
            intensity REAL,
            salience REAL,
            original_salience REAL,
            intent TEXT,
            topics TEXT,
            summary TEXT,
            relationship_impact TEXT,
            contains_code INTEGER,
            language TEXT,
            programming_lang TEXT,
            analyzed_at TEXT,
            analysis_version INTEGER,
            routed_to_heads TEXT,
            last_recalled TEXT,
            recall_count INTEGER,
            contains_error INTEGER,
            error_type TEXT,
            error_severity TEXT,
            error_file TEXT,
            model_version TEXT,
            prompt_tokens INTEGER,
            completion_tokens INTEGER,
            reasoning_tokens INTEGER,
            total_tokens INTEGER,
            latency_ms INTEGER,
            generation_time_ms INTEGER,
            finish_reason TEXT,
            tool_calls TEXT,
            temperature REAL,
            max_tokens INTEGER,
            embedding BLOB,
            embedding_heads TEXT,
            qdrant_point_ids TEXT
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("Failed to create memory_entries table");

    pool
}

// ============================================================================
// RecallContext Tests
// ============================================================================

#[test]
fn test_recall_context_has_code_intelligence_field() {
    // Verify RecallContext includes the code_intelligence field
    let context = RecallContext {
        recent: vec![],
        semantic: vec![],
        rolling_summary: None,
        session_summary: None,
        code_intelligence: None,
    };

    assert!(context.code_intelligence.is_none());
    assert!(context.recent.is_empty());
    assert!(context.semantic.is_empty());
}

#[test]
fn test_recall_context_with_code_intelligence() {
    // Create a mock GatheredContext
    let gathered = GatheredContext::empty();

    let context = RecallContext {
        recent: vec![],
        semantic: vec![],
        rolling_summary: Some("Test summary".to_string()),
        session_summary: None,
        code_intelligence: Some(gathered),
    };

    assert!(context.code_intelligence.is_some());
    assert!(context.rolling_summary.is_some());
}

// ============================================================================
// ContextConfig Tests
// ============================================================================

#[test]
fn test_context_config_presets() {
    let minimal = ContextConfig::minimal();
    assert!(minimal.include_code_search);
    assert!(!minimal.include_call_graph);
    assert!(!minimal.include_expertise);
    assert_eq!(minimal.max_context_tokens, 4000);

    let full = ContextConfig::full();
    assert!(full.include_code_search);
    assert!(full.include_call_graph);
    assert!(full.include_cochange);
    assert!(full.include_historical_fixes);
    assert!(full.include_expertise);
    assert_eq!(full.max_context_tokens, 16000);

    let error = ContextConfig::for_error();
    assert!(error.include_historical_fixes);
    assert!(error.include_build_errors);
    assert!(!error.include_cochange);
}

#[test]
fn test_context_config_default() {
    let config = ContextConfig::default();

    assert!(config.include_code_search);
    assert!(config.include_call_graph);
    assert!(config.include_cochange);
    assert!(config.include_historical_fixes);
    assert!(config.include_patterns);
    assert!(config.include_reasoning_patterns);
    assert!(config.include_build_errors);
    assert!(config.include_expertise); // Expertise enabled by default for better context
    assert!(config.include_error_resolutions); // Error resolutions enabled
    assert!(config.include_semantic_concepts); // Semantic concepts enabled
    assert!(config.include_guidelines); // Guidelines enabled
    assert_eq!(config.max_context_tokens, 8000);
    assert_eq!(config.max_code_results, 10);
}

// ============================================================================
// Budget-Aware Config Tests
// ============================================================================

#[test]
fn test_context_config_for_budget_full() {
    // Low usage (<40%) should return full config
    let config = ContextConfig::for_budget(20.0, 30.0);

    assert!(config.include_expertise); // Full config includes expertise
    assert!(config.include_cochange);
    assert_eq!(config.max_context_tokens, 16000);
    assert_eq!(config.max_code_results, 20);
}

#[test]
fn test_context_config_for_budget_standard() {
    // Moderate usage (40-80%) should return standard (default) config
    let config = ContextConfig::for_budget(50.0, 60.0);

    // Standard config now includes all features (same as default)
    assert!(config.include_expertise); // Expertise now enabled by default
    assert!(config.include_cochange);
    assert!(config.include_guidelines); // Guidelines always included
    assert_eq!(config.max_context_tokens, 8000);
    assert_eq!(config.max_code_results, 10);
}

#[test]
fn test_context_config_for_budget_minimal() {
    // High usage (>80%) should return minimal config
    let config = ContextConfig::for_budget(85.0, 70.0);

    assert!(!config.include_expertise);
    assert!(!config.include_cochange);
    assert!(!config.include_call_graph);
    assert_eq!(config.max_context_tokens, 4000);
    assert_eq!(config.max_code_results, 5);
}

#[test]
fn test_context_config_for_budget_uses_more_restrictive() {
    // Should use monthly (90%) not daily (30%)
    let config = ContextConfig::for_budget(30.0, 90.0);

    // Should be minimal due to monthly being >80%
    assert!(!config.include_call_graph);
    assert_eq!(config.max_context_tokens, 4000);
}

#[test]
fn test_context_config_for_error_with_budget() {
    // Comfortable budget: full error config
    let config = ContextConfig::for_error_with_budget(20.0, 30.0);
    assert!(config.include_call_graph);
    assert!(config.include_historical_fixes);
    assert_eq!(config.max_context_tokens, 12000);

    // Moderate budget: reduced error config
    let config = ContextConfig::for_error_with_budget(70.0, 50.0);
    assert!(config.include_call_graph);
    assert!(config.include_historical_fixes);
    assert_eq!(config.max_context_tokens, 8000);

    // Critical budget: minimal error config
    let config = ContextConfig::for_error_with_budget(95.0, 80.0);
    assert!(!config.include_call_graph);
    assert!(config.include_historical_fixes); // Always keep historical fixes for errors
    assert_eq!(config.max_context_tokens, 4000);
}

// ============================================================================
// BudgetStatus Tests
// ============================================================================

use mira_backend::context_oracle::BudgetStatus;

#[test]
fn test_budget_status_creation() {
    let status = BudgetStatus::new(2.50, 5.00, 75.00, 150.00);

    assert_eq!(status.daily_usage_percent, 50.0);
    assert_eq!(status.monthly_usage_percent, 50.0);
    assert_eq!(status.daily_spent_usd, 2.50);
    assert_eq!(status.daily_limit_usd, 5.00);
    assert_eq!(status.monthly_spent_usd, 75.00);
    assert_eq!(status.monthly_limit_usd, 150.00);
}

#[test]
fn test_budget_status_get_config() {
    // Low usage - should get full config
    let status = BudgetStatus::new(1.00, 5.00, 30.00, 150.00);
    let config = status.get_config();
    assert!(config.include_expertise);
    assert_eq!(config.max_context_tokens, 16000);

    // High usage - should get minimal config
    let status = BudgetStatus::new(4.50, 5.00, 140.00, 150.00);
    let config = status.get_config();
    assert!(!config.include_expertise);
    assert_eq!(config.max_context_tokens, 4000);
}

#[test]
fn test_budget_status_is_critical() {
    let status = BudgetStatus::new(4.60, 5.00, 50.00, 150.00);
    assert!(status.is_critical()); // 92% daily

    let status = BudgetStatus::new(2.00, 5.00, 140.00, 150.00);
    assert!(status.is_critical()); // 93% monthly

    let status = BudgetStatus::new(2.00, 5.00, 50.00, 150.00);
    assert!(!status.is_critical()); // Both < 90%
}

#[test]
fn test_budget_status_is_low() {
    let status = BudgetStatus::new(3.60, 5.00, 50.00, 150.00);
    assert!(status.is_low()); // 72% daily

    let status = BudgetStatus::new(2.00, 5.00, 110.00, 150.00);
    assert!(status.is_low()); // 73% monthly

    let status = BudgetStatus::new(2.00, 5.00, 50.00, 150.00);
    assert!(!status.is_low()); // Both < 70%
}

#[test]
fn test_budget_status_remaining() {
    let status = BudgetStatus::new(3.00, 5.00, 100.00, 150.00);

    assert_eq!(status.daily_remaining(), 2.00);
    assert_eq!(status.monthly_remaining(), 50.00);

    // Over budget should return 0
    let status = BudgetStatus::new(6.00, 5.00, 160.00, 150.00);
    assert_eq!(status.daily_remaining(), 0.00);
    assert_eq!(status.monthly_remaining(), 0.00);
}

// ============================================================================
// Context Oracle Tests
// ============================================================================

#[tokio::test]
async fn test_context_oracle_creation() {
    let pool = setup_test_pool().await;
    let oracle = ContextOracle::new(Arc::new(pool));

    // Should create successfully without any services
    // Oracle uses builder pattern to add services
    let _ = oracle;
}

#[tokio::test]
async fn test_context_oracle_empty_gather() {
    let pool = setup_test_pool().await;
    let oracle = ContextOracle::new(Arc::new(pool));

    use mira_backend::context_oracle::ContextRequest;
    let request = ContextRequest::new("test query".to_string(), "session-1".to_string());

    let context = oracle.gather(&request).await.unwrap();

    // Should return empty context when no services configured
    assert!(context.is_empty());
    assert!(context.sources_used.is_empty());
    assert_eq!(context.estimated_tokens, 0);
}

// ============================================================================
// GatheredContext Tests
// ============================================================================

#[test]
fn test_gathered_context_empty() {
    let context = GatheredContext::empty();

    assert!(context.is_empty());
    assert!(context.code_context.is_none());
    assert!(context.call_graph.is_none());
    assert!(context.cochange_suggestions.is_empty());
    assert!(context.historical_fixes.is_empty());
    assert!(context.design_patterns.is_empty());
    assert!(context.reasoning_patterns.is_empty());
    assert!(context.build_errors.is_empty());
    assert!(context.expertise.is_empty());
    assert_eq!(context.estimated_tokens, 0);
    assert_eq!(context.duration_ms, 0);
}

#[test]
fn test_gathered_context_format_for_prompt_empty() {
    let context = GatheredContext::empty();
    let formatted = context.format_for_prompt();

    // Empty context should produce empty string
    assert!(formatted.is_empty());
}

// ============================================================================
// RecallConfig Tests
// ============================================================================

#[test]
fn test_recall_config_default() {
    let config = RecallConfig::default();

    assert_eq!(config.recent_count, 10);
    assert_eq!(config.semantic_count, 20);
    assert_eq!(config.k_per_head, 10);
    assert_eq!(config.recency_weight, 0.3);
    assert_eq!(config.similarity_weight, 0.5);
    assert_eq!(config.salience_weight, 0.2);
}

#[test]
fn test_recall_config_custom() {
    let config = RecallConfig {
        recent_count: 5,
        semantic_count: 15,
        k_per_head: 8,
        recency_weight: 0.4,
        similarity_weight: 0.4,
        salience_weight: 0.2,
    };

    assert_eq!(config.recent_count, 5);
    assert_eq!(config.semantic_count, 15);
}
