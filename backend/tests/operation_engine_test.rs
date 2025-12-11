// tests/operation_engine_test.rs
// UPDATED: Rewritten to test operation engine through public API
//
// Tests operation lifecycle, event emission, and database state tracking
// without relying on internal lifecycle methods.

use mira_backend::api::ws::message::SystemAccessMode;
use mira_backend::config::CONFIG;
use mira_backend::git::client::GitClient;
use mira_backend::git::store::GitStore;
use mira_backend::llm::provider::LlmProvider;
use mira_backend::llm::provider::{OpenAIEmbeddings, OpenAIProvider};
use mira_backend::llm::router::{ModelRouter, RouterConfig};
use mira_backend::memory::features::code_intelligence::CodeIntelligenceService;
use mira_backend::memory::service::MemoryService;
use mira_backend::memory::storage::qdrant::multi_store::QdrantMultiStore;
use mira_backend::memory::storage::sqlite::store::SqliteMemoryStore;
use mira_backend::operations::engine::{OperationEngine, OperationEngineEvent};
use mira_backend::relationship::facts_service::FactsService;
use mira_backend::relationship::service::RelationshipService;

use sqlx::SqlitePool;
use sqlx::sqlite::SqlitePoolOptions;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;

mod common;

fn create_test_llm() -> Arc<dyn LlmProvider> {
    Arc::new(
        OpenAIProvider::gpt51(common::openai_api_key())
            .expect("Should create LLM provider")
    )
}

async fn setup_services(
    pool: Arc<SqlitePool>,
) -> (
    Arc<MemoryService>,
    Arc<RelationshipService>,
    GitClient,
    Arc<CodeIntelligenceService>,
) {
    // Create SQLite store
    let sqlite_store = Arc::new(SqliteMemoryStore::new((*pool).clone()));

    // Create MultiHeadMemoryStore
    let multi_store = Arc::new(
        QdrantMultiStore::new(&CONFIG.qdrant_url, "test_ops")
            .await
            .expect("Failed to connect to Qdrant - ensure Qdrant is running on port 6334"),
    );

    // Create embedding client with real API key
    let embedding_client = Arc::new(OpenAIEmbeddings::new(
        common::openai_api_key(),
    ));

    // Create LLM provider for MemoryService
    let llm_provider = create_test_llm();

    // Create MemoryService
    let memory_service = Arc::new(MemoryService::new(
        sqlite_store,
        multi_store.clone(),
        llm_provider.clone(),
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

async fn test_operation_engine_lifecycle() {
    println!("\n=== Testing Operation Engine Lifecycle ===\n");

    // Setup in-memory database
    let pool = SqlitePoolOptions::new()
        .connect(":memory:")
        .await
        .expect("Failed to create in-memory database");

    // Run migrations
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("Failed to run migrations");

    let db = Arc::new(pool);
    let llm = create_test_llm();

    let (memory_service, relationship_service, git_client, code_intelligence) =
        setup_services(db.clone()).await;

    // Create model router for tests (use same provider for all 4 tiers)
    let model_router = Arc::new(ModelRouter::new(
        llm.clone(),
        llm.clone(),
        llm.clone(),
        llm.clone(),
        RouterConfig::default(),
    ));

    let engine = OperationEngine::new(
        db.clone(),
        llm.clone(),
        model_router,
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
        None, // session_cache_store
        None, // mcp_manager
    );

    // Create event channel
    let (tx, mut rx) = mpsc::channel(100);

    // Create operation
    let op = engine
        .create_operation(
            "test-session-123".to_string(),
            "code_generation".to_string(),
            "Create a hello world function".to_string(),
        )
        .await
        .expect("Failed to create operation");

    println!("✓ Created operation: {}", op.id);
    assert_eq!(op.session_id, "test-session-123");
    assert_eq!(op.kind, "code_generation");
    assert_eq!(op.status, "pending");
    assert!(op.started_at.is_none());
    assert!(op.completed_at.is_none());

    // Run operation (will fail with fake keys, but lifecycle will be tracked)
    let _result = engine
        .run_operation(
            &op.id,
            "test-session-123",
            "Create a hello world function",
            None, // no project
            SystemAccessMode::Project,
            None, // no cancel token
            &tx,
        )
        .await;

    println!("✓ Operation executed");

    // Drain and verify events
    let mut got_started = false;
    let mut got_status_change = false;
    let mut got_completed_or_failed = false;

    while let Ok(event) = rx.try_recv() {
        match event {
            OperationEngineEvent::Started { operation_id } => {
                assert_eq!(operation_id, op.id);
                got_started = true;
                println!("✓ Received Started event");
            }
            OperationEngineEvent::StatusChanged {
                operation_id,
                old_status,
                new_status,
            } => {
                assert_eq!(operation_id, op.id);
                got_status_change = true;
                println!("✓ Status change: {} -> {}", old_status, new_status);
            }
            OperationEngineEvent::Failed {
                operation_id,
                error,
            } => {
                assert_eq!(operation_id, op.id);
                got_completed_or_failed = true;
                println!("✓ Received Failed event: {}", error);
            }
            OperationEngineEvent::Completed { operation_id, .. } => {
                assert_eq!(operation_id, op.id);
                got_completed_or_failed = true;
                println!("✓ Received Completed event");
            }
            _ => {}
        }
    }

    assert!(got_started, "Should have received Started event");
    assert!(
        got_status_change,
        "Should have received StatusChanged event"
    );
    assert!(got_completed_or_failed, "Should have completed or failed");

    // Verify database state
    let updated_op = engine
        .get_operation(&op.id)
        .await
        .expect("Failed to get operation");

    assert_ne!(updated_op.status, "pending", "Status should have changed");
    assert!(
        updated_op.started_at.is_some(),
        "Should have started_at timestamp"
    );
    assert!(
        updated_op.completed_at.is_some(),
        "Should have completed_at timestamp"
    );
    println!("✓ Database state verified: status = {}", updated_op.status);

    // Check events in database
    let events = engine
        .get_operation_events(&op.id)
        .await
        .expect("Failed to get events");

    assert!(!events.is_empty(), "Should have events in database");
    println!("✓ Events stored in database: {} events", events.len());

    // Verify sequence numbers are sequential
    for (i, event) in events.iter().enumerate() {
        assert_eq!(
            event.sequence_number, i as i64,
            "Event sequence numbers should be sequential starting from 0"
        );
    }
    println!("✓ Event sequence numbers are correct");

    println!("\n✅ Operation lifecycle test passed!\n");
}

#[tokio::test]

async fn test_operation_cancellation() {
    println!("\n=== Testing Operation Cancellation ===\n");

    let pool = SqlitePoolOptions::new()
        .connect(":memory:")
        .await
        .expect("Failed to create in-memory database");

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("Failed to run migrations");

    let db = Arc::new(pool);
    let llm = create_test_llm();

    let (memory_service, relationship_service, git_client, code_intelligence) =
        setup_services(db.clone()).await;

    // Create model router for tests (use same provider for all 4 tiers)
    let model_router = Arc::new(ModelRouter::new(
        llm.clone(),
        llm.clone(),
        llm.clone(),
        llm.clone(),
        RouterConfig::default(),
    ));

    let engine = OperationEngine::new(
        db.clone(),
        llm.clone(),
        model_router,
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
        None, // session_cache_store
        None, // mcp_manager
    );

    let (tx, mut rx) = mpsc::channel(100);

    // Create operation
    let op = engine
        .create_operation(
            "test-session-456".to_string(),
            "code_generation".to_string(),
            "This will be cancelled".to_string(),
        )
        .await
        .expect("Failed to create operation");

    println!("✓ Created operation: {}", op.id);

    // Create a pre-cancelled token
    let cancel_token = tokio_util::sync::CancellationToken::new();
    cancel_token.cancel();

    // Run with cancelled token
    let result = engine
        .run_operation(
            &op.id,
            "test-session-456",
            "This will be cancelled",
            None,
            SystemAccessMode::Project,
            Some(cancel_token),
            &tx,
        )
        .await;

    assert!(result.is_err(), "Should fail due to cancellation");
    println!("✓ Operation cancelled as expected");

    // Verify we got a failure event with cancellation message
    let mut got_cancelled_error = false;
    while let Ok(event) = rx.try_recv() {
        if let OperationEngineEvent::Failed { error, .. } = event {
            if error.contains("cancel") {
                got_cancelled_error = true;
                println!("✓ Cancellation error: {}", error);
            }
        }
    }

    assert!(
        got_cancelled_error,
        "Should have received cancellation error"
    );

    // Verify database state shows failure
    let updated_op = engine
        .get_operation(&op.id)
        .await
        .expect("Failed to get operation");

    assert_eq!(updated_op.status, "failed");
    assert!(updated_op.error.is_some());
    println!("✓ Database shows failed status");

    println!("\n✅ Cancellation test passed!\n");
}

#[tokio::test]

async fn test_multiple_operations() {
    println!("\n=== Testing Multiple Operations ===\n");

    let pool = SqlitePoolOptions::new()
        .connect(":memory:")
        .await
        .expect("Failed to create in-memory database");

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("Failed to run migrations");

    let db = Arc::new(pool);
    let llm = create_test_llm();

    let (memory_service, relationship_service, git_client, code_intelligence) =
        setup_services(db.clone()).await;

    // Create model router for tests (use same provider for all 4 tiers)
    let model_router = Arc::new(ModelRouter::new(
        llm.clone(),
        llm.clone(),
        llm.clone(),
        llm.clone(),
        RouterConfig::default(),
    ));

    let engine = OperationEngine::new(
        db.clone(),
        llm.clone(),
        model_router,
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
        None, // session_cache_store
        None, // mcp_manager
    );

    // Create multiple operations
    let mut operation_ids = Vec::new();

    for i in 0..5 {
        let (tx, _rx) = mpsc::channel(100);
        let session_id = format!("session-{}", i);

        let op = engine
            .create_operation(
                session_id.clone(),
                "code_generation".to_string(),
                format!("Operation {}", i),
            )
            .await
            .expect("Failed to create operation");

        operation_ids.push(op.id.clone());

        // Run operation (will fail with fake keys)
        let _ = engine
            .run_operation(
                &op.id,
                &session_id,
                &format!("Operation {}", i),
                None,
                SystemAccessMode::Project,
                None,
                &tx,
            )
            .await;

        println!("✓ Completed operation {}", i);
    }

    // Verify all operations were tracked
    for (i, op_id) in operation_ids.iter().enumerate() {
        let op = engine
            .get_operation(op_id)
            .await
            .expect("Failed to get operation");

        assert_ne!(op.status, "pending", "Operation {} should have run", i);
        assert!(
            op.started_at.is_some(),
            "Operation {} should have started",
            i
        );
        println!("✓ Operation {} verified: status = {}", i, op.status);
    }

    println!("\n✅ Multiple operations test passed!\n");
}

#[tokio::test]

async fn test_operation_event_ordering() {
    println!("\n=== Testing Operation Event Ordering ===\n");

    let pool = SqlitePoolOptions::new()
        .connect(":memory:")
        .await
        .expect("Failed to create in-memory database");

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("Failed to run migrations");

    let db = Arc::new(pool);
    let llm = create_test_llm();

    let (memory_service, relationship_service, git_client, code_intelligence) =
        setup_services(db.clone()).await;

    // Create model router for tests (use same provider for all 4 tiers)
    let model_router = Arc::new(ModelRouter::new(
        llm.clone(),
        llm.clone(),
        llm.clone(),
        llm.clone(),
        RouterConfig::default(),
    ));

    let engine = OperationEngine::new(
        db.clone(),
        llm.clone(),
        model_router,
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
        None, // session_cache_store
        None, // mcp_manager
    );

    let (tx, _rx) = mpsc::channel(100);

    let op = engine
        .create_operation(
            "test-session".to_string(),
            "code_generation".to_string(),
            "Test event ordering".to_string(),
        )
        .await
        .expect("Failed to create operation");

    // Run operation
    let _ = engine
        .run_operation(
            &op.id,
            "test-session",
            "Test event ordering",
            None,
            SystemAccessMode::Project,
            None,
            &tx,
        )
        .await;

    println!("✓ Operation executed");

    // Get events from database
    let events = engine
        .get_operation_events(&op.id)
        .await
        .expect("Failed to get events");

    assert!(!events.is_empty(), "Should have events");
    println!("✓ Found {} events", events.len());

    // Verify events are ordered by sequence number
    for i in 0..events.len() - 1 {
        assert!(
            events[i].sequence_number < events[i + 1].sequence_number,
            "Events should be ordered by sequence number"
        );
    }
    println!("✓ Events are properly ordered");

    // Verify sequence numbers start at 0 and are contiguous
    for (i, event) in events.iter().enumerate() {
        assert_eq!(
            event.sequence_number, i as i64,
            "Sequence numbers should be contiguous starting from 0"
        );
    }
    println!("✓ Sequence numbers are contiguous");

    println!("\n✅ Event ordering test passed!\n");
}

#[tokio::test]

async fn test_operation_retrieval() {
    println!("\n=== Testing Operation Retrieval ===\n");

    let pool = SqlitePoolOptions::new()
        .connect(":memory:")
        .await
        .expect("Failed to create in-memory database");

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("Failed to run migrations");

    let db = Arc::new(pool);
    let llm = create_test_llm();

    let (memory_service, relationship_service, git_client, code_intelligence) =
        setup_services(db.clone()).await;

    // Create model router for tests (use same provider for all 4 tiers)
    let model_router = Arc::new(ModelRouter::new(
        llm.clone(),
        llm.clone(),
        llm.clone(),
        llm.clone(),
        RouterConfig::default(),
    ));

    let engine = OperationEngine::new(
        db.clone(),
        llm.clone(),
        model_router,
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
        None, // session_cache_store
        None, // mcp_manager
    );

    // Create operation with specific fields
    let op = engine
        .create_operation(
            "session-abc".to_string(),
            "refactoring".to_string(),
            "Refactor authentication module".to_string(),
        )
        .await
        .expect("Failed to create operation");

    println!("✓ Created operation: {}", op.id);

    // Retrieve operation
    let retrieved = engine
        .get_operation(&op.id)
        .await
        .expect("Failed to retrieve operation");

    // Verify all fields match
    assert_eq!(retrieved.id, op.id);
    assert_eq!(retrieved.session_id, "session-abc");
    assert_eq!(retrieved.kind, "refactoring");
    assert_eq!(retrieved.user_message, "Refactor authentication module");
    assert_eq!(retrieved.status, "pending");
    println!("✓ Retrieved operation matches created operation");

    println!("\n✅ Operation retrieval test passed!\n");
}
