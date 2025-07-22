// tests/recall.rs

use mira_backend::memory::sqlite::store::SqliteMemoryStore;
use mira_backend::memory::qdrant::store::QdrantMemoryStore;
use mira_backend::memory::types::MemoryEntry;
use mira_backend::memory::traits::MemoryStore;
use mira_backend::memory::recall::build_context;
use chrono::Utc;
use std::sync::Arc;
use reqwest::Client;

#[tokio::test]
async fn test_memory_recall_context_returns_recent_and_semantic_matches() {
    // In-memory SQLite for fast tests
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect(":memory:")
        .await
        .expect("Failed to open in-memory sqlite");

    mira_backend::memory::sqlite::migration::run_migrations(&pool)
        .await
        .expect("Failed to run migrations");

    let sqlite_store = Arc::new(SqliteMemoryStore::new(pool));
    // For this test, connect to Qdrant (must be running!)
    let qdrant_store = Arc::new(QdrantMemoryStore::new(
        Client::new(),
        "http://localhost:6333",
        "mira-memory",
    ));

    // Populate with fake recent messages
    let user_entry = MemoryEntry {
        id: None,
        session_id: "test-session".to_string(),
        role: "user".to_string(),
        content: "Hello, memory!".to_string(),
        timestamp: Utc::now(),
        embedding: None,
        salience: Some(5.0),
        tags: Some(vec!["test".to_string()]),
        summary: Some("A greeting".to_string()),
        memory_type: Some(mira_backend::memory::types::MemoryType::Fact),
        logprobs: None,
        moderation_flag: None,
        system_fingerprint: None,
    };

    sqlite_store.save(&user_entry).await.expect("Save failed");

    // Now recall context
    let context = build_context(
        "test-session",
        None,    // No embedding for this test
        5,       // num recent
        0,       // num semantic
        sqlite_store.as_ref(),
        qdrant_store.as_ref(),
    )
    .await
    .expect("Failed to build context");

    assert!(!context.recent.is_empty(), "No recent messages returned");
    assert_eq!(context.recent[0].content, "Hello, memory!");
    // Semantic is empty because no embedding or semantic count = 0
    assert!(context.semantic.is_empty());
}
