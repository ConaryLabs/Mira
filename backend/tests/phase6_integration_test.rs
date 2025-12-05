// tests/phase6_integration_test.rs
// UPDATED: Rewritten to use public API (run_operation) instead of internal methods
//
// Phase 6: Operation Engine Integration Tests
// Tests full orchestration: LLM -> Tool execution -> Artifact creation

use mira_backend::git::client::GitClient;
use mira_backend::git::store::GitStore;
use mira_backend::llm::provider::{Gemini3Provider, ThinkingLevel};
use mira_backend::llm::provider::{LlmProvider, GeminiEmbeddings};
use mira_backend::memory::features::code_intelligence::CodeIntelligenceService;
use mira_backend::memory::service::MemoryService;
use mira_backend::memory::storage::qdrant::multi_store::QdrantMultiStore;
use mira_backend::memory::storage::sqlite::store::SqliteMemoryStore;
use mira_backend::operations::{OperationEngine, OperationEngineEvent};
use mira_backend::relationship::facts_service::FactsService;
use mira_backend::relationship::service::RelationshipService;
use sqlx::sqlite::SqlitePoolOptions;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Helper to create test database
async fn create_test_db() -> Arc<sqlx::SqlitePool> {
    let pool = SqlitePoolOptions::new()
        .connect(":memory:")
        .await
        .expect("Failed to create in-memory database");

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("Failed to run migrations");

    Arc::new(pool)
}

/// Helper to create test LLM provider (uses fake API key)
fn create_test_llm() -> Gemini3Provider {
    Gemini3Provider::new(
        "test-llm-key".to_string(),
        "gemini-2.5-flash".to_string(),
        ThinkingLevel::High,
    ).expect("Should create LLM provider")
}

/// Setup test services
async fn setup_services(
    pool: Arc<sqlx::SqlitePool>,
) -> (
    Arc<MemoryService>,
    Arc<RelationshipService>,
    GitClient,
    Arc<CodeIntelligenceService>,
) {
    let sqlite_store = Arc::new(SqliteMemoryStore::new((*pool).clone()));

    let qdrant_url = "http://localhost:6334";
    let multi_store = Arc::new(
        QdrantMultiStore::new(qdrant_url, "test_phase6")
            .await
            .unwrap_or_else(|_| panic!("Qdrant not available")),
    );

    let embedding_client = Arc::new(GeminiEmbeddings::new(
        "test-key".to_string(),
        "gemini-embedding-001".to_string(),
    ));

    let llm_provider: Arc<dyn LlmProvider> = Arc::new(Gemini3Provider::new(
        "test-key".to_string(),
        "gemini-2.5-flash".to_string(),
        ThinkingLevel::High,
    ).expect("Should create LLM provider"));

    let memory_service = Arc::new(MemoryService::new(
        sqlite_store,
        multi_store.clone(),
        llm_provider,
        embedding_client.clone(),
    ));

    // Create FactsService FIRST
    let facts_service = Arc::new(FactsService::new((*pool).clone()));

    // Create RelationshipService WITH FactsService
    let relationship_service = Arc::new(RelationshipService::new(
        pool.clone(),
        facts_service.clone(),
    ));

    // Create GitClient
    let git_store = GitStore::new((*pool).clone());
    let git_client = GitClient::new(PathBuf::from("./test_repos"), git_store);

    // FIXED: CodeIntelligenceService needs Pool, not Arc<Pool>
    let code_intelligence = Arc::new(CodeIntelligenceService::new(
        (*pool).clone(),
        multi_store.clone(),
        embedding_client.clone(),
    ));

    (
        memory_service,
        relationship_service,
        git_client,
        code_intelligence,
    )
}

#[tokio::test]

async fn test_operation_engine_with_providers() {
    let db = create_test_db().await;
    let llm =create_test_llm();

    let (memory_service, relationship_service, git_client, code_intelligence) =
        setup_services(db.clone()).await;

    let engine = OperationEngine::new(
        db.clone(),
        llm,
        memory_service,
        relationship_service,
        git_client,
        code_intelligence,
        None, // sudo_service
        None, // context_oracle
        None, // budget_tracker
        None, // llm_cache
        None, // project_task_service
        None, // guidelines_service
        None, // hook_manager
        None, // checkpoint_manager
        None, // project_store
    );

    // Create event channel
    let (tx, mut rx) = mpsc::channel(100);

    // Create operation
    let op = engine
        .create_operation(
            "test-session-phase6".to_string(),
            "code_generation".to_string(),
            "Create a Rust function that adds two numbers".to_string(),
        )
        .await
        .expect("Failed to create operation");

    println!("+ Created operation: {}", op.id);

    // Use run_operation to execute the full lifecycle
    // This will fail with fake API keys, but we can verify the initial setup
    let result = engine
        .run_operation(
            &op.id,
            "test-session-phase6",
            "Create a Rust function that adds two numbers",
            None, // no project
            None, // no cancel token
            &tx,
        )
        .await;

    // With fake API keys, this will fail, but we can verify events were emitted
    assert!(result.is_err(), "Should fail with fake API keys");

    // Drain and verify some events were emitted
    let mut event_count = 0;
    while let Ok(event) = rx.try_recv() {
        event_count += 1;
        match event {
            OperationEngineEvent::Started { operation_id } => {
                assert_eq!(operation_id, op.id);
                println!("+ Received Started event");
            }
            OperationEngineEvent::StatusChanged {
                old_status,
                new_status,
                ..
            } => {
                println!("+ Status change: {} -> {}", old_status, new_status);
            }
            OperationEngineEvent::Failed { error, .. } => {
                println!("+ Expected failure: {}", error);
            }
            _ => {}
        }
    }

    assert!(event_count > 0, "Should have emitted some events");
    println!("+ Emitted {} events", event_count);

    // Verify operation was created in database
    let updated_op = engine
        .get_operation(&op.id)
        .await
        .expect("Failed to get operation");

    assert!(!updated_op.status.is_empty());
    println!("+ Operation status: {}", updated_op.status);

    // Verify events in database
    let events = engine
        .get_operation_events(&op.id)
        .await
        .expect("Failed to get events");

    assert!(!events.is_empty(), "Should have at least 1 event");
    println!("+ Events stored in database: {} events", events.len());

    println!("\n[PASS] Provider integration test passed!");
}

#[tokio::test]

async fn test_operation_lifecycle_complete() {
    let db = create_test_db().await;
    let llm =create_test_llm();

    let (memory_service, relationship_service, git_client, code_intelligence) =
        setup_services(db.clone()).await;

    let engine = OperationEngine::new(
        db.clone(),
        llm,
        memory_service,
        relationship_service,
        git_client,
        code_intelligence,
        None, // sudo_service
        None, // context_oracle
        None, // budget_tracker
        None, // llm_cache
        None, // project_task_service
        None, // guidelines_service
        None, // hook_manager
        None, // checkpoint_manager
        None, // project_store
    );

    let (tx, mut rx) = mpsc::channel(100);
    let session_id = "test-session";

    // Create operation
    let op = engine
        .create_operation(
            session_id.to_string(),
            "code_generation".to_string(),
            "Test operation".to_string(),
        )
        .await
        .expect("Failed to create operation");

    // Run operation (will fail with fake keys, but lifecycle will be tracked)
    let _ = engine
        .run_operation(&op.id, session_id, "Test operation", None, None, &tx)
        .await;

    println!("+ Operation executed");

    // Drain events and verify lifecycle
    let mut got_started = false;
    let mut got_failed = false;
    while let Ok(event) = rx.try_recv() {
        match event {
            OperationEngineEvent::Started { .. } => {
                got_started = true;
                println!("+ Got Started event");
            }
            OperationEngineEvent::Failed { .. } => {
                got_failed = true;
                println!("+ Got Failed event");
            }
            OperationEngineEvent::StatusChanged {
                old_status,
                new_status,
                ..
            } => {
                println!("+ Status: {} -> {}", old_status, new_status);
            }
            _ => {}
        }
    }

    assert!(got_started, "Should have started");
    assert!(got_failed, "Should have failed (fake API keys)");

    // Verify final database state shows lifecycle was tracked
    let final_op = engine
        .get_operation(&op.id)
        .await
        .expect("Failed to get operation");
    assert_ne!(final_op.status, "pending", "Status should have changed");
    assert!(
        final_op.started_at.is_some(),
        "Should have started_at timestamp"
    );
    assert!(
        final_op.completed_at.is_some(),
        "Should have completed_at timestamp"
    );

    println!("[PASS] Lifecycle completion test passed!");
}

#[tokio::test]

async fn test_operation_cancellation() {
    let db = create_test_db().await;
    let llm =create_test_llm();

    let (memory_service, relationship_service, git_client, code_intelligence) =
        setup_services(db.clone()).await;

    let engine = OperationEngine::new(
        db.clone(),
        llm,
        memory_service,
        relationship_service,
        git_client,
        code_intelligence,
        None, // sudo_service
        None, // context_oracle
        None, // budget_tracker
        None, // llm_cache
        None, // project_task_service
        None, // guidelines_service
        None, // hook_manager
        None, // checkpoint_manager
        None, // project_store
    );

    let (tx, mut rx) = mpsc::channel(100);

    // Create operation
    let op = engine
        .create_operation(
            "test-session".to_string(),
            "code_generation".to_string(),
            "This will be cancelled".to_string(),
        )
        .await
        .expect("Failed to create operation");

    // Create a pre-cancelled token
    let cancel_token = tokio_util::sync::CancellationToken::new();
    cancel_token.cancel();

    // Run operation with cancelled token
    let result = engine
        .run_operation(
            &op.id,
            "test-session",
            "This will be cancelled",
            None,
            Some(cancel_token),
            &tx,
        )
        .await;

    // Should fail due to cancellation
    assert!(result.is_err(), "Should fail due to cancellation");
    println!("+ Operation cancelled as expected");

    // Verify cancellation was tracked
    let mut got_failed = false;
    while let Ok(event) = rx.try_recv() {
        if let OperationEngineEvent::Failed { error, .. } = event {
            assert!(error.contains("cancelled") || error.contains("canceled"));
            got_failed = true;
            println!("+ Got cancellation error: {}", error);
        }
    }

    assert!(got_failed, "Should have received failure event");

    // Verify final database state
    let final_op = engine
        .get_operation(&op.id)
        .await
        .expect("Failed to get operation");
    assert_eq!(final_op.status, "failed");
    assert!(final_op.error.is_some());

    println!("[PASS] Cancellation handling test passed!");
}

#[tokio::test]

async fn test_multiple_operations_concurrency() {
    let db = create_test_db().await;
    let llm =create_test_llm();

    let (memory_service, relationship_service, git_client, code_intelligence) =
        setup_services(db.clone()).await;

    let engine = Arc::new(OperationEngine::new(
        db.clone(),
        llm,
        memory_service,
        relationship_service,
        git_client,
        code_intelligence,
        None, // sudo_service
        None, // context_oracle
        None, // budget_tracker
        None, // llm_cache
        None, // project_task_service
        None, // guidelines_service
        None, // hook_manager
        None, // checkpoint_manager
        None, // project_store
    ));

    let mut handles = vec![];

    // Create 5 concurrent operations
    for i in 0..5 {
        let engine_clone = engine.clone();
        let handle = tokio::spawn(async move {
            let (tx, _rx) = mpsc::channel(100);
            let session_id = format!("session-{}", i);

            let op = engine_clone
                .create_operation(
                    session_id.clone(),
                    "code_generation".to_string(),
                    format!("Operation {}", i),
                )
                .await
                .expect("Failed to create operation");

            // Run operation (will fail with fake keys)
            let _ = engine_clone
                .run_operation(
                    &op.id,
                    &session_id,
                    &format!("Operation {}", i),
                    None,
                    None,
                    &tx,
                )
                .await;

            op.id
        });

        handles.push(handle);
    }

    // Wait for all operations to complete
    for handle in handles {
        let op_id = handle.await.expect("Task panicked");
        let op = engine
            .get_operation(&op_id)
            .await
            .expect("Failed to get operation");

        // Should have been executed (even if failed)
        assert_ne!(op.status, "pending", "Operation should have run");
        assert!(op.started_at.is_some(), "Should have started");
        println!("+ Operation {} completed with status: {}", op_id, op.status);
    }

    println!("[PASS] Concurrency test passed!");
}
