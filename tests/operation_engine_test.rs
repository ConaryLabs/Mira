// tests/operation_engine_test.rs
// FIXED: Updated for OperationEngine 7-parameter constructor

use mira_backend::config::CONFIG;
use mira_backend::llm::provider::OpenAiEmbeddings;
use mira_backend::llm::provider::LlmProvider;
use mira_backend::llm::provider::gpt5::Gpt5Provider;
use mira_backend::llm::provider::deepseek::DeepSeekProvider;
use mira_backend::memory::service::MemoryService;
use mira_backend::memory::storage::sqlite::store::SqliteMemoryStore;
use mira_backend::memory::storage::qdrant::multi_store::QdrantMultiStore;
use mira_backend::memory::features::code_intelligence::CodeIntelligenceService;
use mira_backend::operations::engine::{OperationEngine, OperationEngineEvent};
use mira_backend::relationship::service::RelationshipService;
use mira_backend::relationship::facts_service::FactsService;
use mira_backend::git::store::GitStore;
use mira_backend::git::client::GitClient;

use sqlx::SqlitePool;
use sqlx::sqlite::SqlitePoolOptions;
use std::sync::Arc;
use std::path::PathBuf;
use tokio::sync::mpsc;

fn create_test_providers() -> (Gpt5Provider, DeepSeekProvider) {
    let gpt5 = Gpt5Provider::new(
        "test-key".to_string(),
        "gpt-5-preview".to_string(),
        4000,
        "medium".to_string(),
        "medium".to_string(),
    );

    let deepseek = DeepSeekProvider::new("test-key".to_string());

    (gpt5, deepseek)
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
        QdrantMultiStore::new(
            &CONFIG.qdrant_url,
            "test_ops"
        )
        .await
        .unwrap_or_else(|_| {
            panic!("Qdrant not available - these tests don't actually need it")
        })
    );
    
    // Create embedding client (won't be used in these tests)
    let embedding_client = Arc::new(OpenAiEmbeddings::new(
        "test-key".to_string(),
        "text-embedding-3-large".to_string(),
    ));
    
    // Create LLM provider for MemoryService
    let llm_provider: Arc<dyn LlmProvider> = Arc::new(Gpt5Provider::new(
        "test-key".to_string(),
        "gpt-5-preview".to_string(),
        4000,
        "medium".to_string(),
        "medium".to_string(),
    ));
    
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
    
    // NEW: Create GitClient
    let git_store = GitStore::new((*pool).clone());
    let git_client = GitClient::new(
        PathBuf::from("./test_repos"),
        git_store,
    );
    
    // NEW: Create CodeIntelligenceService
    let code_intelligence = Arc::new(CodeIntelligenceService::new(
        pool.clone(),
        multi_store.clone(),
        embedding_client.clone(),
    ));
    
    (memory_service, relationship_service, git_client, code_intelligence)
}

#[tokio::test]
async fn test_operation_engine_lifecycle() {
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
    let (gpt5, deepseek) = create_test_providers();
    
    // Setup services - NOW RETURNS 4 SERVICES
    let (memory_service, relationship_service, git_client, code_intelligence) = 
        setup_services(db.clone()).await;
    
    // FIXED: Add git_client and code_intelligence parameters
    let engine = OperationEngine::new(
        db.clone(),
        gpt5,
        deepseek,
        memory_service,
        relationship_service,
        git_client,
        code_intelligence,
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

    println!("+ Created operation: {}", op.id);
    assert_eq!(op.session_id, "test-session-123");
    assert_eq!(op.kind, "code_generation");
    assert_eq!(op.status, "pending");
    assert!(op.started_at.is_none());
    assert!(op.completed_at.is_none());

    // Start operation
    engine
        .start_operation(&op.id, &tx)
        .await
        .expect("Failed to start operation");

    println!("+ Started operation");

    // Check events
    let event = rx.recv().await.expect("No Started event received");
    match event {
        OperationEngineEvent::Started { operation_id } => {
            assert_eq!(operation_id, op.id);
            println!("+ Received Started event");
        }
        _ => panic!("Expected Started event, got: {:?}", event),
    }

    let event = rx.recv().await.expect("No StatusChanged event received");
    match event {
        OperationEngineEvent::StatusChanged {
            operation_id,
            old_status,
            new_status,
        } => {
            assert_eq!(operation_id, op.id);
            assert_eq!(old_status, "pending");
            assert_eq!(new_status, "planning");
            println!("+ Received StatusChanged event (pending -> planning)");
        }
        _ => panic!("Expected StatusChanged event, got: {:?}", event),
    }

    // Complete operation
    engine
        .complete_operation(&op.id, "test-session-123", Some("Success! Generated code.".to_string()), &tx)
        .await
        .expect("Failed to complete operation");

    println!("+ Completed operation");

    // Check completion events
    let event = rx.recv().await.expect("No StatusChanged event received");
    match event {
        OperationEngineEvent::StatusChanged {
            operation_id,
            old_status,
            new_status,
        } => {
            assert_eq!(operation_id, op.id);
            assert_eq!(old_status, "planning");
            assert_eq!(new_status, "completed");
            println!("+ Received StatusChanged event (planning -> completed)");
        }
        _ => panic!("Expected StatusChanged event, got: {:?}", event),
    }

    let event = rx.recv().await.expect("No Completed event received");
    match event {
        OperationEngineEvent::Completed {
            operation_id,
            result,
            artifacts,
        } => {
            assert_eq!(operation_id, op.id);
            assert_eq!(result.unwrap(), "Success! Generated code.");
            println!("+ Received Completed event (with {} artifacts)", artifacts.len());
        }
        _ => panic!("Expected Completed event, got: {:?}", event),
    }

    // Verify database state
    let updated_op = engine
        .get_operation(&op.id)
        .await
        .expect("Failed to get operation");

    assert_eq!(updated_op.status, "completed");
    assert!(updated_op.started_at.is_some());
    assert!(updated_op.completed_at.is_some());
    assert_eq!(
        updated_op.result.unwrap(),
        "Success! Generated code."
    );
    println!("+ Database state verified");

    // Check events in database
    let events = engine
        .get_operation_events(&op.id)
        .await
        .expect("Failed to get events");

    assert!(events.len() >= 2, "Should have at least 2 events");
    println!("+ Events stored in database: {} events", events.len());

    // Verify sequence numbers
    for (i, event) in events.iter().enumerate() {
        assert_eq!(
            event.sequence_number,
            i as i64,
            "Event sequence numbers should be sequential starting from 0"
        );
    }
    println!("+ Event sequence numbers are correct");

    println!("\n[PASS] All tests passed!");
}

#[tokio::test]
async fn test_operation_failure() {
    let pool = SqlitePoolOptions::new()
        .connect(":memory:")
        .await
        .expect("Failed to create in-memory database");

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("Failed to run migrations");

    let db = Arc::new(pool);
    let (gpt5, deepseek) = create_test_providers();
    
    // Setup services - NOW RETURNS 4 SERVICES
    let (memory_service, relationship_service, git_client, code_intelligence) = 
        setup_services(db.clone()).await;
    
    // FIXED: Add git_client and code_intelligence parameters
    let engine = OperationEngine::new(
        db.clone(),
        gpt5,
        deepseek,
        memory_service,
        relationship_service,
        git_client,
        code_intelligence,
    );
    let (tx, mut rx) = mpsc::channel(100);

    // Create and start operation
    let op = engine
        .create_operation(
            "test-session-456".to_string(),
            "code_generation".to_string(),
            "This will fail".to_string(),
        )
        .await
        .expect("Failed to create operation");

    engine
        .start_operation(&op.id, &tx)
        .await
        .expect("Failed to start operation");

    // Drain started events
    let _ = rx.recv().await;
    let _ = rx.recv().await;

    // Fail the operation
    engine
        .fail_operation(
            &op.id,
            "DeepSeek API error: timeout".to_string(),
            &tx,
        )
        .await
        .expect("Failed to fail operation");

    println!("+ Failed operation");

    // Check events
    let event = rx.recv().await.expect("No StatusChanged event received");
    match event {
        OperationEngineEvent::StatusChanged {
            operation_id,
            old_status,
            new_status,
        } => {
            assert_eq!(operation_id, op.id);
            assert_eq!(old_status, "planning");
            assert_eq!(new_status, "failed");
            println!("+ Received StatusChanged event (planning -> failed)");
        }
        _ => panic!("Expected StatusChanged event, got: {:?}", event),
    }

    let event = rx.recv().await.expect("No Failed event received");
    match event {
        OperationEngineEvent::Failed {
            operation_id,
            error,
        } => {
            assert_eq!(operation_id, op.id);
            assert_eq!(error, "DeepSeek API error: timeout");
            println!("+ Received Failed event");
        }
        _ => panic!("Expected Failed event, got: {:?}", event),
    }

    // Verify database
    let updated_op = engine
        .get_operation(&op.id)
        .await
        .expect("Failed to get operation");

    assert_eq!(updated_op.status, "failed");
    assert!(updated_op.started_at.is_some());
    assert!(updated_op.completed_at.is_some());
    assert_eq!(
        updated_op.error.unwrap(),
        "DeepSeek API error: timeout"
    );
    println!("+ Database state verified");

    println!("\n[PASS] Failure test passed!");
}

#[tokio::test]
async fn test_multiple_operations() {
    let pool = SqlitePoolOptions::new()
        .connect(":memory:")
        .await
        .expect("Failed to create in-memory database");

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("Failed to run migrations");

    let db = Arc::new(pool);
    let (gpt5, deepseek) = create_test_providers();
    
    // Setup services - NOW RETURNS 4 SERVICES
    let (memory_service, relationship_service, git_client, code_intelligence) = 
        setup_services(db.clone()).await;
    
    // FIXED: Add git_client and code_intelligence parameters
    let engine = OperationEngine::new(
        db.clone(),
        gpt5,
        deepseek,
        memory_service,
        relationship_service,
        git_client,
        code_intelligence,
    );
    let (tx, _rx) = mpsc::channel(100);

    // Create multiple operations
    for i in 0..5 {
        let session_id = format!("session-{}", i);
        let op = engine
            .create_operation(
                session_id.clone(),
                "code_generation".to_string(),
                format!("Operation {}", i),
            )
            .await
            .expect("Failed to create operation");

        engine
            .start_operation(&op.id, &tx)
            .await
            .expect("Failed to start operation");

        engine
            .complete_operation(&op.id, &session_id, Some(format!("Result {}", i)), &tx)
            .await
            .expect("Failed to complete operation");

        println!("+ Completed operation {}", i);
    }

    println!("\n[PASS] Multiple operations test passed!");
}
