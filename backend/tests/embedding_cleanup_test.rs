// tests/embedding_cleanup_test.rs
//
// Tests for the embedding cleanup system
// These are integration tests that require a running Qdrant instance

use mira_backend::{
    memory::storage::qdrant::multi_store::QdrantMultiStore,
    tasks::embedding_cleanup::{CleanupReport, CollectionReport, EmbeddingCleanupTask},
};
use sqlx::sqlite::SqlitePoolOptions;
use std::sync::Arc;

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
    // Note: qdrant-client uses gRPC which defaults to port 6334
    let qdrant_url =
        std::env::var("QDRANT_URL").unwrap_or_else(|_| "http://localhost:6334".to_string());

    Arc::new(
        QdrantMultiStore::new(&qdrant_url, "test_cleanup")
            .await
            .expect("Failed to create Qdrant store"),
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

    report.add_collection(
        "semantic".to_string(),
        CollectionReport {
            checked: 100,
            orphans: 5,
            deleted: 5,
        },
    );

    report.add_collection(
        "code".to_string(),
        CollectionReport {
            checked: 50,
            orphans: 2,
            deleted: 2,
        },
    );

    assert_eq!(report.total_checked, 150);
    assert_eq!(report.orphans_found, 7);
    assert_eq!(report.orphans_deleted, 7);
}

#[tokio::test]
async fn test_cleanup_report_summary() {
    let mut report = CleanupReport::new();

    report.add_collection(
        "semantic".to_string(),
        CollectionReport {
            checked: 100,
            orphans: 5,
            deleted: 5,
        },
    );

    let summary = report.summary();
    assert!(summary.contains("100"));
    assert!(summary.contains("5"));
}

#[tokio::test]

async fn test_cleanup_dry_run() {
    let pool = create_test_db().await;
    let multi_store = create_test_qdrant().await;

    let cleanup = EmbeddingCleanupTask::new(pool, multi_store);

    // Dry run should not fail even with no data
    let report = cleanup.run(true).await.expect("Dry run failed");

    assert_eq!(
        report.orphans_deleted, 0,
        "Dry run should not delete anything"
    );
}

#[tokio::test]

async fn test_cleanup_runs_successfully() {
    let pool = create_test_db().await;
    let multi_store = create_test_qdrant().await;

    let cleanup = EmbeddingCleanupTask::new(pool, multi_store);

    // Run cleanup on empty/fresh collections - should complete without errors
    let report = cleanup.run(false).await.expect("Cleanup failed");

    // Verify report structure is valid
    assert!(report.errors.is_empty(), "Cleanup should not have errors");
}
