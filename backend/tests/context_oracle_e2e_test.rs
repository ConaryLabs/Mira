// tests/context_oracle_e2e_test.rs
// End-to-end tests for Context Oracle with real LLM integration
//
// These tests require:
// - Valid OPENAI_API_KEY in .env
// - Qdrant running on localhost:6334 (gRPC)
//
// Run with: cargo test --test context_oracle_e2e_test -- --ignored --nocapture

mod common;

use chrono::Utc;
use mira_backend::budget::BudgetTracker;
use mira_backend::context_oracle::{ContextConfig, ContextOracle, ContextRequest};
use mira_backend::llm::provider::{LlmProvider, OpenAIEmbeddings, OpenAIProvider};
use mira_backend::memory::service::MemoryService;
use mira_backend::memory::storage::qdrant::multi_store::QdrantMultiStore;
use mira_backend::memory::storage::sqlite::store::SqliteMemoryStore;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;
use std::sync::Arc;

// ============================================================================
// Test Helpers
// ============================================================================

async fn setup_test_pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(":memory:")
        .await
        .expect("Failed to create in-memory pool");

    // Run migrations
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("Failed to run migrations");

    pool
}

fn get_openai_api_key() -> String {
    common::openai_api_key()
}

fn get_qdrant_url() -> String {
    std::env::var("QDRANT_URL").unwrap_or_else(|_| "http://localhost:6334".to_string())
}

// ============================================================================
// End-to-End Tests (require real services)
// ============================================================================

#[tokio::test]
async fn test_context_oracle_full_flow() {
    let api_key = get_openai_api_key();

    let pool = setup_test_pool().await;
    let qdrant_url = get_qdrant_url();

    // Initialize services
    let multi_store = Arc::new(
        QdrantMultiStore::new(&qdrant_url, "test_e2e")
            .await
            .expect("Failed to connect to Qdrant"),
    );

    let embedding_client = Arc::new(OpenAIEmbeddings::new(
        api_key.clone(),
    ));

    let code_intelligence = Arc::new(
        mira_backend::memory::features::code_intelligence::CodeIntelligenceService::new(
            pool.clone(),
            multi_store.clone(),
            embedding_client.clone(),
        ),
    );

    // Create Context Oracle with code intelligence
    let oracle = Arc::new(
        ContextOracle::new(Arc::new(pool.clone()))
            .with_code_intelligence(code_intelligence.clone()),
    );

    // Create a test request
    let request = ContextRequest::new(
        "How do I implement error handling in Rust?".to_string(),
        "test-session-e2e".to_string(),
    )
    .with_config(ContextConfig::minimal());

    // Gather context
    let result = oracle.gather(&request).await;
    assert!(result.is_ok(), "Oracle gather should succeed");

    let context = result.unwrap();
    println!("Gathered context sources: {:?}", context.sources_used);
    println!("Estimated tokens: {}", context.estimated_tokens);
    println!("Duration: {}ms", context.duration_ms);

    // Context may be empty if no code is indexed, but should not error
}

#[tokio::test]
async fn test_memory_service_with_oracle() {
    let api_key = get_openai_api_key();

    let pool = setup_test_pool().await;
    let qdrant_url = get_qdrant_url();

    // Initialize stores
    let sqlite_store = Arc::new(SqliteMemoryStore::new(pool.clone()));
    let multi_store = Arc::new(
        QdrantMultiStore::new(&qdrant_url, "test_memory_e2e")
            .await
            .expect("Failed to connect to Qdrant"),
    );

    // Initialize providers
    let llm_provider: Arc<dyn LlmProvider> = Arc::new(
        OpenAIProvider::gpt51(api_key.clone())
            .expect("Failed to create LLM provider"),
    );
    let embedding_client = Arc::new(OpenAIEmbeddings::new(
        api_key.clone(),
    ));

    let code_intelligence = Arc::new(
        mira_backend::memory::features::code_intelligence::CodeIntelligenceService::new(
            pool.clone(),
            multi_store.clone(),
            embedding_client.clone(),
        ),
    );

    // Create Context Oracle
    let oracle = Arc::new(
        ContextOracle::new(Arc::new(pool.clone()))
            .with_code_intelligence(code_intelligence.clone()),
    );

    // Create MemoryService with oracle
    let memory_service = MemoryService::with_oracle(
        sqlite_store.clone(),
        multi_store.clone(),
        llm_provider.clone(),
        embedding_client.clone(),
        Some(oracle.clone()),
    );

    assert!(memory_service.has_oracle(), "Memory service should have oracle");

    // Save a test message
    let session_id = "test-session-memory-e2e";
    memory_service
        .save_user_message(session_id, "Test message for e2e", None)
        .await
        .expect("Failed to save message");

    // Build enriched context
    let result = memory_service
        .build_enriched_context(session_id, "test query", None, None)
        .await;

    assert!(result.is_ok(), "build_enriched_context should succeed");

    let context = result.unwrap();
    println!("Recent messages: {}", context.recent.len());
    println!("Semantic matches: {}", context.semantic.len());
    println!(
        "Code intelligence: {}",
        context.code_intelligence.is_some()
    );
}

#[tokio::test]
async fn test_budget_aware_config_with_tracker() {
    let _api_key = get_openai_api_key();

    let pool = setup_test_pool().await;

    // Create a test user first (required for foreign key constraint)
    let now = Utc::now().timestamp();
    sqlx::query("INSERT INTO users (id, username, email, password_hash, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?)")
        .bind("test-user-e2e")
        .bind("test_user_e2e")
        .bind("test@example.com")
        .bind("placeholder_hash")
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .expect("Failed to create test user");

    // Create budget tracker with $5/day, $150/month limits
    let tracker = BudgetTracker::new(pool.clone(), 5.0, 150.0);

    let user_id = "test-user-e2e";

    // Record some spending
    tracker
        .record_request(
            user_id,
            None,
            "openai",
            "gpt-5.1",
            Some("medium"),
            1000,
            500,
            0, // tokens_cached
            0.05,
            false,
        )
        .await
        .expect("Failed to record request");

    // Get budget status
    let status = tracker
        .get_budget_status(user_id)
        .await
        .expect("Failed to get budget status");

    println!("Daily usage: {:.1}%", status.daily_usage_percent);
    println!("Monthly usage: {:.1}%", status.monthly_usage_percent);
    println!("Daily remaining: ${:.2}", status.daily_remaining());
    println!("Monthly remaining: ${:.2}", status.monthly_remaining());

    // Get appropriate config based on budget
    let config = status.get_config();
    println!(
        "Selected config - max tokens: {}, code results: {}",
        config.max_context_tokens, config.max_code_results
    );

    // Verify config matches budget state
    // With $0.05 spent of $5.00 daily (1%), should get full config
    assert!(
        status.daily_usage_percent < 40.0,
        "Daily usage should be low"
    );
    assert_eq!(
        config.max_context_tokens, 16000,
        "Should get full config for low budget usage"
    );
}

#[tokio::test]
async fn test_full_integration_flow() {
    let api_key = get_openai_api_key();

    let pool = setup_test_pool().await;
    let qdrant_url = get_qdrant_url();

    println!("=== Full Integration Test ===");
    println!("1. Setting up services...");

    // Initialize all services
    let sqlite_store = Arc::new(SqliteMemoryStore::new(pool.clone()));
    let multi_store = Arc::new(
        QdrantMultiStore::new(&qdrant_url, "test_full_e2e")
            .await
            .expect("Failed to connect to Qdrant"),
    );

    let llm_provider: Arc<dyn LlmProvider> = Arc::new(
        OpenAIProvider::gpt51(api_key.clone())
            .expect("Failed to create LLM provider"),
    );
    let embedding_client = Arc::new(OpenAIEmbeddings::new(
        api_key.clone(),
    ));

    let code_intelligence = Arc::new(
        mira_backend::memory::features::code_intelligence::CodeIntelligenceService::new(
            pool.clone(),
            multi_store.clone(),
            embedding_client.clone(),
        ),
    );

    // Git intelligence services
    let cochange_service = Arc::new(mira_backend::git::intelligence::CochangeService::new(
        pool.clone(),
    ));
    let expertise_service = Arc::new(mira_backend::git::intelligence::ExpertiseService::new(
        pool.clone(),
    ));
    let fix_service = Arc::new(mira_backend::git::intelligence::FixService::new(
        pool.clone(),
    ));

    // Build tracker
    let build_tracker = Arc::new(mira_backend::build::BuildTracker::new(Arc::new(
        pool.clone(),
    )));

    // Pattern services
    let pattern_storage = Arc::new(mira_backend::patterns::PatternStorage::new(Arc::new(
        pool.clone(),
    )));
    let pattern_matcher = Arc::new(mira_backend::patterns::PatternMatcher::new(
        pattern_storage.clone(),
    ));

    println!("2. Creating Context Oracle with all services...");

    // Create fully-configured Context Oracle
    let oracle = Arc::new(
        ContextOracle::new(Arc::new(pool.clone()))
            .with_code_intelligence(code_intelligence.clone())
            .with_cochange(cochange_service.clone())
            .with_expertise(expertise_service.clone())
            .with_fix_service(fix_service.clone())
            .with_build_tracker(build_tracker.clone())
            .with_pattern_storage(pattern_storage.clone())
            .with_pattern_matcher(pattern_matcher.clone()),
    );

    println!("3. Creating MemoryService with oracle...");

    // Create MemoryService with oracle
    let memory_service = MemoryService::with_oracle(
        sqlite_store.clone(),
        multi_store.clone(),
        llm_provider.clone(),
        embedding_client.clone(),
        Some(oracle.clone()),
    );

    println!("4. Testing budget-aware config selection...");

    // Test budget-aware config selection
    let budget_tracker = BudgetTracker::new(pool.clone(), 5.0, 150.0);
    let budget_status = budget_tracker
        .get_budget_status("test-user")
        .await
        .expect("Failed to get budget status");

    let config = budget_status.get_config();
    println!(
        "   Budget status: {:.1}% daily, {:.1}% monthly",
        budget_status.daily_usage_percent, budget_status.monthly_usage_percent
    );
    println!(
        "   Selected config: {} tokens, {} code results",
        config.max_context_tokens, config.max_code_results
    );

    println!("5. Testing context gathering...");

    // Save a test message and build context
    let session_id = "test-full-integration";
    memory_service
        .save_user_message(session_id, "How do I handle database errors?", None)
        .await
        .expect("Failed to save message");

    let context = memory_service
        .build_enriched_context_with_config(
            session_id,
            "database error handling",
            config,
            None,
            None,
            Some("database connection failed"),
        )
        .await
        .expect("Failed to build enriched context");

    println!("6. Results:");
    println!("   Recent messages: {}", context.recent.len());
    println!("   Semantic matches: {}", context.semantic.len());
    println!(
        "   Code intelligence: {}",
        if context.code_intelligence.is_some() {
            "present"
        } else {
            "none"
        }
    );

    if let Some(ci) = &context.code_intelligence {
        println!("   - Sources used: {:?}", ci.sources_used);
        println!("   - Estimated tokens: {}", ci.estimated_tokens);
    }

    println!("=== Integration Test Complete ===");
}
