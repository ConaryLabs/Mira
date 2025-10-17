// tests/phase6_integration_test.rs
//
// Phase 6: Operation Engine Integration Tests
// Tests full orchestration: GPT-5 → DeepSeek delegation → Artifact creation

use mira_backend::operations::{OperationEngine, OperationEngineEvent};
use mira_backend::llm::provider::gpt5::Gpt5Provider;
use mira_backend::llm::provider::deepseek::DeepSeekProvider;
use sqlx::sqlite::SqlitePoolOptions;
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

/// Helper to create test providers (these will use fake API keys for now)
fn create_test_providers() -> (Gpt5Provider, DeepSeekProvider) {
    let gpt5 = Gpt5Provider::new(
        "test-gpt5-key".to_string(),
        "gpt-5-preview".to_string(),
        4000,
        "medium".to_string(),
        "medium".to_string(),
    );

    let deepseek = DeepSeekProvider::new("test-deepseek-key".to_string());

    (gpt5, deepseek)
}

#[tokio::test]
async fn test_operation_engine_with_providers() {
    let db = create_test_db().await;
    let (gpt5, deepseek) = create_test_providers();
    let engine = OperationEngine::new(db.clone(), gpt5, deepseek);

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

    println!("✓ Created operation: {}", op.id);

    // Start operation
    engine
        .start_operation(&op.id, &tx)
        .await
        .expect("Failed to start operation");

    println!("✓ Started operation");

    // Verify Started event
    let event = rx.recv().await.expect("No Started event received");
    match event {
        OperationEngineEvent::Started { operation_id } => {
            assert_eq!(operation_id, op.id);
            println!("✓ Received Started event");
        }
        _ => panic!("Expected Started event, got: {:?}", event),
    }

    // Verify StatusChanged event (pending → planning)
    let event = rx.recv().await.expect("No StatusChanged event received");
    match event {
        OperationEngineEvent::StatusChanged {
            old_status,
            new_status,
            ..
        } => {
            assert_eq!(old_status, "pending");
            assert_eq!(new_status, "planning");
            println!("✓ Received StatusChanged event (pending → planning)");
        }
        _ => panic!("Expected StatusChanged event, got: {:?}", event),
    }

    // Verify database state
    let updated_op = engine
        .get_operation(&op.id)
        .await
        .expect("Failed to get operation");

    assert_eq!(updated_op.status, "planning");
    assert!(updated_op.started_at.is_some());
    println!("✓ Database state verified");

    // Verify events in database
    let events = engine
        .get_operation_events(&op.id)
        .await
        .expect("Failed to get events");

    assert!(!events.is_empty(), "Should have at least 1 event");
    println!("✓ Events stored in database: {} events", events.len());

    println!("\n✅ Provider integration test passed!");
}

#[tokio::test]
async fn test_operation_lifecycle_complete() {
    let db = create_test_db().await;
    let (gpt5, deepseek) = create_test_providers();
    let engine = OperationEngine::new(db.clone(), gpt5, deepseek);

    let (tx, mut rx) = mpsc::channel(100);

    // Create and start operation
    let op = engine
        .create_operation(
            "test-session".to_string(),
            "code_generation".to_string(),
            "Test operation".to_string(),
        )
        .await
        .expect("Failed to create operation");

    engine
        .start_operation(&op.id, &tx)
        .await
        .expect("Failed to start operation");

    // Drain initial events
    let _ = rx.recv().await; // Started
    let _ = rx.recv().await; // StatusChanged

    // Complete operation
    engine
        .complete_operation(&op.id, Some("Success!".to_string()), &tx)
        .await
        .expect("Failed to complete operation");

    println!("✓ Completed operation");

    // Verify completion events
    let event = rx.recv().await.expect("No StatusChanged event");
    match event {
        OperationEngineEvent::StatusChanged {
            old_status,
            new_status,
            ..
        } => {
            assert_eq!(old_status, "planning");
            assert_eq!(new_status, "completed");
        }
        _ => panic!("Expected StatusChanged event"),
    }

    let event = rx.recv().await.expect("No Completed event");
    match event {
        OperationEngineEvent::Completed { result, .. } => {
            assert_eq!(result.unwrap(), "Success!");
        }
        _ => panic!("Expected Completed event"),
    }

    // Verify final database state
    let final_op = engine.get_operation(&op.id).await.expect("Failed to get operation");
    assert_eq!(final_op.status, "completed");
    assert!(final_op.completed_at.is_some());
    assert_eq!(final_op.result.unwrap(), "Success!");

    println!("✅ Lifecycle completion test passed!");
}

#[tokio::test]
async fn test_operation_failure_handling() {
    let db = create_test_db().await;
    let (gpt5, deepseek) = create_test_providers();
    let engine = OperationEngine::new(db.clone(), gpt5, deepseek);

    let (tx, mut rx) = mpsc::channel(100);

    // Create and start operation
    let op = engine
        .create_operation(
            "test-session".to_string(),
            "code_generation".to_string(),
            "This will fail".to_string(),
        )
        .await
        .expect("Failed to create operation");

    engine
        .start_operation(&op.id, &tx)
        .await
        .expect("Failed to start operation");

    // Drain initial events
    let _ = rx.recv().await;
    let _ = rx.recv().await;

    // Fail operation
    engine
        .fail_operation(&op.id, "Test error".to_string(), &tx)
        .await
        .expect("Failed to fail operation");

    println!("✓ Failed operation");

    // Verify failure events
    let event = rx.recv().await.expect("No StatusChanged event");
    match event {
        OperationEngineEvent::StatusChanged {
            old_status,
            new_status,
            ..
        } => {
            assert_eq!(old_status, "planning");
            assert_eq!(new_status, "failed");
        }
        _ => panic!("Expected StatusChanged event"),
    }

    let event = rx.recv().await.expect("No Failed event");
    match event {
        OperationEngineEvent::Failed { error, .. } => {
            assert_eq!(error, "Test error");
        }
        _ => panic!("Expected Failed event"),
    }

    // Verify final database state
    let final_op = engine.get_operation(&op.id).await.expect("Failed to get operation");
    assert_eq!(final_op.status, "failed");
    assert!(final_op.completed_at.is_some());
    assert_eq!(final_op.error.unwrap(), "Test error");

    println!("✅ Failure handling test passed!");
}

#[tokio::test]
async fn test_multiple_operations_concurrency() {
    let db = create_test_db().await;
    let (gpt5, deepseek) = create_test_providers();
    let engine = Arc::new(OperationEngine::new(db.clone(), gpt5, deepseek));

    let mut handles = vec![];

    // Create 5 concurrent operations
    for i in 0..5 {
        let engine_clone = engine.clone();
        let handle = tokio::spawn(async move {
            let (tx, _rx) = mpsc::channel(100);

            let op = engine_clone
                .create_operation(
                    format!("session-{}", i),
                    "code_generation".to_string(),
                    format!("Operation {}", i),
                )
                .await
                .expect("Failed to create operation");

            engine_clone
                .start_operation(&op.id, &tx)
                .await
                .expect("Failed to start operation");

            engine_clone
                .complete_operation(&op.id, Some(format!("Result {}", i)), &tx)
                .await
                .expect("Failed to complete operation");

            op.id
        });

        handles.push(handle);
    }

    // Wait for all operations to complete
    for handle in handles {
        let op_id = handle.await.expect("Task panicked");
        let op = engine.get_operation(&op_id).await.expect("Failed to get operation");
        assert_eq!(op.status, "completed");
        println!("✓ Operation {} completed", op_id);
    }

    println!("✅ Concurrency test passed!");
}
