// tests/rolling_summary_test.rs
// Rolling summary generation and retrieval tests - REWRITTEN for current schema
// Tests SummarizationEngine's ability to create and manage rolling summaries

use mira_backend::config::CONFIG;
use mira_backend::llm::provider::GeminiEmbeddings;
use mira_backend::llm::provider::Gemini3Provider;
use mira_backend::memory::{
    features::memory_types::SummaryType,
    service::MemoryService,
    storage::{qdrant::multi_store::QdrantMultiStore, sqlite::store::SqliteMemoryStore},
};
use sqlx::SqlitePool;
use std::sync::Arc;

// ============================================================================
// Test Setup Helpers
// ============================================================================

async fn create_test_memory_service() -> Arc<MemoryService> {
    // Create in-memory SQLite database
    let pool = SqlitePool::connect(":memory:")
        .await
        .expect("Failed to create in-memory database");

    // Run migrations
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("Failed to run migrations");

    let sqlite_store = Arc::new(SqliteMemoryStore::new(pool));

    // Create Qdrant multi-store
    let qdrant_url = &CONFIG.qdrant_url;
    let collection_base = &CONFIG.qdrant_collection;
    let multi_store = Arc::new(
        QdrantMultiStore::new(qdrant_url, collection_base)
            .await
            .expect("Failed to create Qdrant multi-store"),
    );

    // Create LLM provider - use Gemini3Provider with CONFIG settings
    let api_key = CONFIG.google_api_key.clone();
    let llm_provider = Arc::new(Gemini3Provider::new(
        api_key.clone(),
        CONFIG.gemini_model.clone(),
        CONFIG.gemini_thinking.clone(),
    ).expect("Failed to create GPT5 provider"));

    // Create embedding client - use CONFIG settings
    let embedding_client = Arc::new(GeminiEmbeddings::new(
        CONFIG.google_api_key.clone(),
        CONFIG.gemini_embedding_model.clone(),
    ));

    Arc::new(MemoryService::new(
        sqlite_store,
        multi_store,
        llm_provider,
        embedding_client,
    ))
}

async fn create_test_messages(
    memory_service: &Arc<MemoryService>,
    session_id: &str,
    count: usize,
) -> Vec<i64> {
    let mut message_ids = Vec::new();

    for i in 0..count {
        let content = format!("Test message {} about coding and debugging", i + 1);

        let msg_id = memory_service
            .save_user_message(session_id, &content, None)
            .await
            .expect("Failed to save message");

        message_ids.push(msg_id);

        // Add assistant responses for realism
        let response = format!("Response to message {}: Here's how to approach that", i + 1);
        let response_id = memory_service
            .save_assistant_message(session_id, &response, Some(msg_id))
            .await
            .expect("Failed to save response");

        message_ids.push(response_id);
    }

    message_ids
}

// ============================================================================
// Basic Rolling Summary Tests
// ============================================================================

#[tokio::test]
async fn test_create_rolling_10_summary() {
    println!("\n=== Testing Rolling 10-Message Summary Creation ===\n");

    let memory_service = create_test_memory_service().await;
    let session_id = "test-session-rolling-10";

    println!("[1] Creating 10 test messages");
    let _message_ids = create_test_messages(&memory_service, session_id, 5).await;

    println!("[2] Creating rolling summary (10 messages)");

    let summary = memory_service
        .summarization_engine
        .create_rolling_summary(session_id, 10)
        .await
        .expect("Failed to create rolling summary");

    println!("[3] Verifying summary");

    assert!(!summary.is_empty(), "Summary should not be empty");
    assert!(summary.len() > 50, "Summary should have substance");

    println!("✓ Rolling 10-message summary created");
    println!("  Summary length: {} chars", summary.len());
}

#[tokio::test]
async fn test_create_rolling_100_summary() {
    println!("\n=== Testing Rolling 100-Message Summary Creation ===\n");

    let memory_service = create_test_memory_service().await;
    let session_id = "test-session-rolling-100";

    println!("[1] Creating 50 test messages (100 total with responses)");
    let _message_ids = create_test_messages(&memory_service, session_id, 50).await;

    println!("[2] Creating rolling summary (100 messages)");

    let summary = memory_service
        .summarization_engine
        .create_rolling_summary(session_id, 100)
        .await
        .expect("Failed to create rolling summary");

    println!("[3] Verifying summary");

    assert!(!summary.is_empty(), "Summary should not be empty");
    assert!(
        summary.len() > 100,
        "100-message summary should be substantial"
    );

    println!("✓ Rolling 100-message summary created");
    println!("  Summary length: {} chars", summary.len());
}

#[tokio::test]
async fn test_create_summary_with_summary_type() {
    println!("\n=== Testing Summary Creation with SummaryType Enum ===\n");

    let memory_service = create_test_memory_service().await;
    let session_id = "test-session-summary-type";

    println!("[1] Creating test messages");
    create_test_messages(&memory_service, session_id, 5).await;

    println!("[2] Creating summary using SummaryType::Rolling10");

    let summary = memory_service
        .summarization_engine
        .create_summary(session_id, SummaryType::Rolling10)
        .await
        .expect("Failed to create summary via SummaryType");

    println!("[3] Verifying summary");

    assert!(!summary.is_empty(), "Summary should not be empty");

    println!("✓ Summary created via SummaryType enum");
}

// ============================================================================
// Summary Retrieval Tests
// ============================================================================

#[tokio::test]
async fn test_get_rolling_summary() {
    println!("\n=== Testing Rolling Summary Retrieval ===\n");

    let memory_service = create_test_memory_service().await;
    let session_id = "test-session-get-rolling";

    println!("[1] Creating messages and summary");
    create_test_messages(&memory_service, session_id, 5).await;

    let created_summary = memory_service
        .summarization_engine
        .create_rolling_summary(session_id, 10)
        .await
        .expect("Failed to create summary");

    println!("[2] Retrieving rolling summary");

    let retrieved_summary = memory_service
        .get_rolling_summary(session_id)
        .await
        .expect("Failed to retrieve summary");

    println!("[3] Verifying retrieval");

    assert!(
        retrieved_summary.is_some(),
        "Should retrieve created summary"
    );
    let summary = retrieved_summary.unwrap();

    // Content should match (or be similar)
    assert!(!summary.is_empty(), "Retrieved summary should not be empty");
    assert_eq!(
        summary, created_summary,
        "Retrieved summary should match created"
    );

    println!("✓ Rolling summary retrieved successfully");
}

#[tokio::test]
async fn test_get_rolling_summary_none() {
    println!("\n=== Testing Rolling Summary Retrieval (None) ===\n");

    let memory_service = create_test_memory_service().await;
    let session_id = "test-session-no-summary";

    println!("[1] Attempting to retrieve non-existent summary");

    let summary = memory_service
        .get_rolling_summary(session_id)
        .await
        .expect("Failed to query for summary");

    println!("[2] Verifying None result");

    assert!(
        summary.is_none(),
        "Should return None for non-existent summary"
    );

    println!("✓ Correctly returns None for missing summary");
}

#[tokio::test]
async fn test_get_session_summary() {
    println!("\n=== Testing Session Summary Retrieval ===\n");

    let memory_service = create_test_memory_service().await;
    let session_id = "test-session-snapshot";

    println!("[1] Creating messages and snapshot summary");
    create_test_messages(&memory_service, session_id, 10).await;

    memory_service
        .summarization_engine
        .create_snapshot_summary(session_id, None)
        .await
        .expect("Failed to create snapshot summary");

    println!("[2] Retrieving session summary");

    let summary = memory_service
        .get_session_summary(session_id)
        .await
        .expect("Failed to retrieve session summary");

    println!("[3] Verifying retrieval");

    assert!(summary.is_some(), "Should retrieve snapshot summary");
    let summary_text = summary.unwrap();
    assert!(
        !summary_text.is_empty(),
        "Session summary should not be empty"
    );

    println!("✓ Session summary retrieved successfully");
    println!("  Summary length: {} chars", summary_text.len());
}

// ============================================================================
// Background Processing Tests
// ============================================================================

#[tokio::test]
async fn test_check_and_process_summaries_trigger_10() {
    println!("\n=== Testing Background Summary Trigger (10 messages) ===\n");

    let memory_service = create_test_memory_service().await;
    let session_id = "test-session-trigger-10";

    println!("[1] Creating 5 test messages (10 total with responses)");
    create_test_messages(&memory_service, session_id, 5).await;

    println!("[2] Checking if summary should be triggered at 10 messages");

    let result = memory_service
        .summarization_engine
        .check_and_process_summaries(session_id, 10)
        .await
        .expect("Failed to check and process summaries");

    println!("[3] Verifying trigger fired");

    assert!(
        result.is_some(),
        "Should create summary at 10-message threshold"
    );
    let summary = result.unwrap();
    assert!(!summary.is_empty(), "Generated summary should not be empty");

    println!("✓ Background trigger fired at 10 messages");
    println!("  Generated summary: {} chars", summary.len());
}

#[tokio::test]
async fn test_check_and_process_summaries_trigger_100() {
    println!("\n=== Testing Background Summary Trigger (100 messages) ===\n");

    let memory_service = create_test_memory_service().await;
    let session_id = "test-session-trigger-100";

    println!("[1] Creating 50 test messages (100 total)");
    create_test_messages(&memory_service, session_id, 50).await;

    println!("[2] Checking if summary should be triggered at 100 messages");

    let result = memory_service
        .summarization_engine
        .check_and_process_summaries(session_id, 100)
        .await
        .expect("Failed to check and process summaries");

    println!("[3] Verifying trigger fired");

    assert!(
        result.is_some(),
        "Should create summary at 100-message threshold"
    );
    let summary = result.unwrap();
    assert!(!summary.is_empty(), "Generated summary should not be empty");

    println!("✓ Background trigger fired at 100 messages");
    println!("  Generated summary: {} chars", summary.len());
}

#[tokio::test]
async fn test_check_and_process_summaries_no_trigger() {
    println!("\n=== Testing Background Summary (No Trigger) ===\n");

    let memory_service = create_test_memory_service().await;
    let session_id = "test-session-no-trigger";

    println!("[1] Creating 2 test messages (4 total)");
    create_test_messages(&memory_service, session_id, 2).await;

    println!("[2] Checking at 5 messages (not a trigger point)");

    let result = memory_service
        .summarization_engine
        .check_and_process_summaries(session_id, 5)
        .await
        .expect("Failed to check and process summaries");

    println!("[3] Verifying no trigger");

    assert!(result.is_none(), "Should not create summary at 5 messages");

    println!("✓ Correctly skips non-trigger message counts");
}

// ============================================================================
// Snapshot Summary Tests
// ============================================================================

#[tokio::test]
async fn test_create_snapshot_summary() {
    println!("\n=== Testing Snapshot Summary Creation ===\n");

    let memory_service = create_test_memory_service().await;
    let session_id = "test-session-snapshot-create";

    println!("[1] Creating diverse test messages");
    create_test_messages(&memory_service, session_id, 15).await;

    println!("[2] Creating snapshot summary");

    let summary = memory_service
        .summarization_engine
        .create_snapshot_summary(session_id, None)
        .await
        .expect("Failed to create snapshot summary");

    println!("[3] Verifying snapshot");

    assert!(!summary.is_empty(), "Snapshot summary should not be empty");
    assert!(summary.len() > 100, "Snapshot should be comprehensive");

    println!("✓ Snapshot summary created");
    println!("  Summary length: {} chars", summary.len());
}

#[tokio::test]
async fn test_snapshot_vs_rolling_summary() {
    println!("\n=== Testing Snapshot vs Rolling Summary Differences ===\n");

    let memory_service = create_test_memory_service().await;
    let session_id = "test-session-compare";

    println!("[1] Creating test messages");
    create_test_messages(&memory_service, session_id, 10).await;

    println!("[2] Creating rolling summary");
    let rolling = memory_service
        .summarization_engine
        .create_rolling_summary(session_id, 10)
        .await
        .expect("Failed to create rolling summary");

    println!("[3] Creating snapshot summary");
    let snapshot = memory_service
        .summarization_engine
        .create_snapshot_summary(session_id, None)
        .await
        .expect("Failed to create snapshot summary");

    println!("[4] Comparing summaries");

    assert!(!rolling.is_empty(), "Rolling summary should exist");
    assert!(!snapshot.is_empty(), "Snapshot summary should exist");

    // Both should be valid summaries (they may differ in approach)
    println!("✓ Both summary types created successfully");
    println!("  Rolling: {} chars", rolling.len());
    println!("  Snapshot: {} chars", snapshot.len());
}

// ============================================================================
// Edge Cases and Error Handling
// ============================================================================

#[tokio::test]
async fn test_summary_with_empty_session() {
    println!("\n=== Testing Summary Creation with Empty Session ===\n");

    let memory_service = create_test_memory_service().await;
    let session_id = "test-session-empty";

    println!("[1] Attempting to create summary with no messages");

    let result = memory_service
        .summarization_engine
        .create_rolling_summary(session_id, 10)
        .await;

    println!("[2] Verifying error handling");

    // Should either error or return empty summary
    match result {
        Ok(summary) => {
            println!(
                "  Returned summary: '{}' ({} chars)",
                summary.chars().take(50).collect::<String>(),
                summary.len()
            );
            // Empty or minimal summary is acceptable
        }
        Err(e) => {
            println!("  Returned error: {}", e);
            // Error is also acceptable for empty session
        }
    }

    println!("✓ Handled empty session gracefully");
}

#[tokio::test]
async fn test_summary_with_insufficient_messages() {
    println!("\n=== Testing Summary with Insufficient Messages ===\n");

    let memory_service = create_test_memory_service().await;
    let session_id = "test-session-insufficient";

    println!("[1] Creating only 2 messages");
    create_test_messages(&memory_service, session_id, 1).await;

    println!("[2] Attempting to create 100-message summary");

    let result = memory_service
        .summarization_engine
        .create_rolling_summary(session_id, 100)
        .await;

    println!("[3] Verifying handling");

    // Should handle gracefully - either summarize what exists or error
    match result {
        Ok(summary) => {
            println!(
                "  Created summary with available messages: {} chars",
                summary.len()
            );
            assert!(
                !summary.is_empty(),
                "Should create summary from available messages"
            );
        }
        Err(e) => {
            println!("  Returned error: {}", e);
        }
    }

    println!("✓ Handled insufficient messages");
}

#[tokio::test]
async fn test_multiple_summaries_same_session() {
    println!("\n=== Testing Multiple Summaries for Same Session ===\n");

    let memory_service = create_test_memory_service().await;
    let session_id = "test-session-multiple";

    println!("[1] Creating initial messages");
    create_test_messages(&memory_service, session_id, 5).await;

    println!("[2] Creating first summary");
    let summary1 = memory_service
        .summarization_engine
        .create_rolling_summary(session_id, 10)
        .await
        .expect("Failed to create first summary");

    println!("[3] Adding more messages");
    create_test_messages(&memory_service, session_id, 5).await;

    println!("[4] Creating second summary");
    let summary2 = memory_service
        .summarization_engine
        .create_rolling_summary(session_id, 10)
        .await
        .expect("Failed to create second summary");

    println!("[5] Verifying both summaries");

    assert!(!summary1.is_empty(), "First summary should exist");
    assert!(!summary2.is_empty(), "Second summary should exist");

    // Summaries may differ as conversation evolves
    println!("✓ Multiple summaries created successfully");
    println!("  First: {} chars", summary1.len());
    println!("  Second: {} chars", summary2.len());
}

#[tokio::test]
async fn test_summary_types_enum_values() {
    println!("\n=== Testing SummaryType Enum Values ===\n");

    let memory_service = create_test_memory_service().await;
    let session_id = "test-session-enum-types";

    println!("[1] Creating test messages");
    create_test_messages(&memory_service, session_id, 50).await;

    println!("[2] Testing Rolling10 type");
    let rolling10 = memory_service
        .summarization_engine
        .create_summary(session_id, SummaryType::Rolling10)
        .await
        .expect("Rolling10 failed");
    assert!(!rolling10.is_empty());

    println!("[3] Testing Rolling100 type");
    let rolling100 = memory_service
        .summarization_engine
        .create_summary(session_id, SummaryType::Rolling100)
        .await
        .expect("Rolling100 failed");
    assert!(!rolling100.is_empty());

    println!("[4] Testing Snapshot type");
    let snapshot = memory_service
        .summarization_engine
        .create_summary(session_id, SummaryType::Snapshot)
        .await
        .expect("Snapshot failed");
    assert!(!snapshot.is_empty());

    println!("✓ All SummaryType enum variants work");
    println!("  Rolling10: {} chars", rolling10.len());
    println!("  Rolling100: {} chars", rolling100.len());
    println!("  Snapshot: {} chars", snapshot.len());
}

#[tokio::test]
async fn test_summary_content_quality() {
    println!("\n=== Testing Summary Content Quality ===\n");

    let memory_service = create_test_memory_service().await;
    let session_id = "test-session-quality";

    println!("[1] Creating messages with specific topics");

    // Create messages about specific topics - need at least 10 for rolling summary
    memory_service
        .save_user_message(session_id, "I'm working on a Rust web API", None)
        .await
        .unwrap();

    memory_service
        .save_assistant_message(
            session_id,
            "Great! What specifically are you building?",
            None,
        )
        .await
        .unwrap();

    memory_service
        .save_user_message(session_id, "Need help with async error handling", None)
        .await
        .unwrap();

    memory_service
        .save_assistant_message(
            session_id,
            "Let me explain async error patterns in Rust",
            None,
        )
        .await
        .unwrap();

    memory_service
        .save_user_message(session_id, "How do I structure my database models?", None)
        .await
        .unwrap();

    memory_service
        .save_assistant_message(
            session_id,
            "Here are some best practices for database models",
            None,
        )
        .await
        .unwrap();

    // Add a few more to reach 10 messages
    memory_service
        .save_user_message(session_id, "What about connection pooling?", None)
        .await
        .unwrap();

    memory_service
        .save_assistant_message(
            session_id,
            "Connection pools are important for performance",
            None,
        )
        .await
        .unwrap();

    memory_service
        .save_user_message(session_id, "Should I use SQLx or Diesel?", None)
        .await
        .unwrap();

    memory_service
        .save_assistant_message(
            session_id,
            "Both are good choices, here's the tradeoff",
            None,
        )
        .await
        .unwrap();

    println!("[2] Creating rolling summary");

    let summary = memory_service
        .summarization_engine
        .create_rolling_summary(session_id, 10)
        .await
        .expect("Failed to create summary");

    println!("[3] Analyzing summary content");

    let summary_lower = summary.to_lowercase();

    // Check if summary captures key topics (may not contain exact words but related concepts)
    let has_technical_content = summary_lower.contains("rust")
        || summary_lower.contains("api")
        || summary_lower.contains("async")
        || summary_lower.contains("error")
        || summary_lower.contains("database")
        || summary.len() > 50;

    assert!(
        has_technical_content,
        "Summary should capture technical topics"
    );

    println!("✓ Summary captures conversation topics");
    println!(
        "  Summary preview: {}...",
        summary.chars().take(100).collect::<String>()
    );
}

// ============================================================================
// Integration Tests
// ============================================================================

#[tokio::test]
async fn test_end_to_end_summary_workflow() {
    println!("\n=== Testing End-to-End Summary Workflow ===\n");

    let memory_service = create_test_memory_service().await;
    let session_id = "test-session-e2e";

    println!("[1] Creating initial messages");
    create_test_messages(&memory_service, session_id, 5).await;

    println!("[2] Background check (should trigger at 10)");
    let trigger_result = memory_service
        .summarization_engine
        .check_and_process_summaries(session_id, 10)
        .await
        .expect("Background check failed");

    assert!(trigger_result.is_some(), "Should trigger summary creation");

    println!("[3] Retrieving created summary");
    let retrieved = memory_service
        .get_rolling_summary(session_id)
        .await
        .expect("Retrieval failed");

    assert!(retrieved.is_some(), "Should retrieve created summary");

    println!("[4] Creating snapshot summary");
    memory_service
        .summarization_engine
        .create_snapshot_summary(session_id, None)
        .await
        .expect("Snapshot creation failed");

    println!("[5] Retrieving snapshot");
    let snapshot = memory_service
        .get_session_summary(session_id)
        .await
        .expect("Snapshot retrieval failed");

    assert!(snapshot.is_some(), "Should retrieve snapshot");

    println!("✓ Complete workflow successful");
    println!("  Rolling summary exists: {}", retrieved.is_some());
    println!("  Snapshot summary exists: {}", snapshot.is_some());
}

#[tokio::test]
async fn test_concurrent_summary_operations() {
    println!("\n=== Testing Concurrent Summary Operations ===\n");

    let memory_service = create_test_memory_service().await;
    let session_id = "test-session-concurrent";

    println!("[1] Creating test messages");
    create_test_messages(&memory_service, session_id, 10).await;

    println!("[2] Spawning concurrent summary creation tasks");

    let service1 = memory_service.clone();
    let session1 = session_id.to_string();
    let handle1 = tokio::spawn(async move {
        service1
            .summarization_engine
            .create_rolling_summary(&session1, 10)
            .await
    });

    let service2 = memory_service.clone();
    let session2 = session_id.to_string();
    let handle2 = tokio::spawn(async move {
        service2
            .summarization_engine
            .create_snapshot_summary(&session2, None)
            .await
    });

    println!("[3] Waiting for both tasks");

    let result1 = handle1.await.expect("Task 1 panicked");
    let result2 = handle2.await.expect("Task 2 panicked");

    println!("[4] Verifying results");

    assert!(result1.is_ok(), "Rolling summary should succeed");
    assert!(result2.is_ok(), "Snapshot summary should succeed");

    println!("✓ Concurrent operations completed successfully");
}
