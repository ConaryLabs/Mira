// tests/embedding_cleanup_test.rs
//
// Tests for the embedding cleanup system
// These are integration tests that require a running Qdrant instance

use std::sync::Arc;
use sqlx::sqlite::SqlitePoolOptions;
use mira_backend::{
    tasks::embedding_cleanup::{EmbeddingCleanupTask, CleanupReport, CollectionReport},
    memory::storage::qdrant::multi_store::QdrantMultiStore,
};

/// Helper to create in-memory test database
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

/// Helper to create test Qdrant store
async fn create_test_qdrant() -> Arc<QdrantMultiStore> {
    let qdrant_url = std::env::var("QDRANT_URL")
        .unwrap_or_else(|_| "http://localhost:6333".to_string());
    
    Arc::new(
        QdrantMultiStore::new(&qdrant_url, "test_cleanup")
            .await
            .expect("Failed to create Qdrant store")
    )
}

#[tokio::test]
async fn test_cleanup_report_construction() {
    let mut report = CleanupReport::new();
    
    let collection_report = CollectionReport {
        checked: 100,
        orphans: 5,
        deleted: 5,
    };
    
    report.add_collection("semantic".to_string(), collection_report);
    
    assert_eq!(report.total_checked, 100);
    assert_eq!(report.orphans_found, 5);
    assert_eq!(report.orphans_deleted, 5);
    assert_eq!(report.errors.len(), 0);
    assert!(report.by_collection.contains_key("semantic"));
}

#[tokio::test]
async fn test_cleanup_report_multiple_collections() {
    let mut report = CleanupReport::new();
    
    report.add_collection("semantic".to_string(), CollectionReport {
        checked: 100,
        orphans: 5,
        deleted: 5,
    });
    
    report.add_collection("code".to_string(), CollectionReport {
        checked: 50,
        orphans: 2,
        deleted: 2,
    });
    
    assert_eq!(report.total_checked, 150);
    assert_eq!(report.orphans_found, 7);
    assert_eq!(report.orphans_deleted, 7);
}

#[tokio::test]
async fn test_cleanup_report_summary() {
    let mut report = CleanupReport::new();
    
    report.add_collection("semantic".to_string(), CollectionReport {
        checked: 100,
        orphans: 5,
        deleted: 5,
    });
    
    let summary = report.summary();
    assert!(summary.contains("100"));
    assert!(summary.contains("5"));
}

#[tokio::test]
#[ignore] // Requires running Qdrant instance
async fn test_cleanup_dry_run() {
    let pool = create_test_db().await;
    let multi_store = create_test_qdrant().await;
    
    let cleanup = EmbeddingCleanupTask::new(pool, multi_store);
    
    // Dry run should not fail even with no data
    let report = cleanup.run(true).await.expect("Dry run failed");
    
    assert_eq!(report.orphans_deleted, 0, "Dry run should not delete anything");
}

#[tokio::test]
#[ignore] // Requires running Qdrant instance and test data setup
async fn test_cleanup_finds_orphans() {
    let pool = create_test_db().await;
    let multi_store = create_test_qdrant().await;
    
    // Setup: Create a message in SQLite, add to Qdrant, then delete from SQLite
    // This creates an orphan that cleanup should find
    
    // TODO: Implement full test with data setup
    // 1. Insert message into SQLite
    // 2. Add corresponding embedding to Qdrant
    // 3. Delete message from SQLite
    // 4. Run cleanup
    // 5. Verify orphan was found and deleted
    
    let cleanup = EmbeddingCleanupTask::new(pool, multi_store);
    let report = cleanup.run(false).await.expect("Cleanup failed");
    
    // Should find the orphan we created
    assert!(report.orphans_found > 0, "Should have found test orphan");
}

#[tokio::test]
#[ignore] // Requires running Qdrant instance
async fn test_cleanup_preserves_valid_entries() {
    // TODO: Implement test that verifies cleanup doesn't delete valid entries
    // 1. Insert messages into SQLite
    // 2. Add corresponding embeddings to Qdrant
    // 3. Run cleanup
    // 4. Verify all entries still exist in Qdrant
}

// Add more integration tests as needed
