use mira_backend::memory::qdrant::store::QdrantMemoryStore;
use mira_backend::memory::traits::MemoryStore;
use mira_backend::memory::types::{MemoryEntry, MemoryType};
use reqwest::Client;
use chrono::Utc;
use std::env;
use tokio::time::{sleep, Duration};

fn test_embedding() -> Vec<f32> {
    vec![0.123; 1536]
}

#[tokio::test]
async fn qdrant_save_and_recall_roundtrip() {
    println!("🔬 Starting Qdrant Sprint 1 integration test...");

    let base_url = env::var("QDRANT_URL").unwrap_or_else(|_| "http://localhost:6333".to_string());
    let collection = env::var("QDRANT_COLLECTION").unwrap_or_else(|_| "mira-memory".to_string());
    let store = QdrantMemoryStore::new(Client::new(), base_url, collection);

    let now = Utc::now();

    let test_entry = MemoryEntry {
        id: Some(202407182359),
        session_id: "sprint1-integration-test".to_string(),
        role: "user".to_string(),
        content: "🚀 Sprint 1 integration test — Hello, Qdrant memory!".to_string(),
        timestamp: now,
        embedding: Some(test_embedding()),
        salience: Some(9.9),
        tags: Some(vec!["integration".to_string(), "test".to_string()]),
        summary: Some("Testing Qdrant round-trip.".to_string()),
        memory_type: Some(MemoryType::Fact),
        logprobs: None,
        moderation_flag: Some(false),
        system_fingerprint: Some("integration-test".to_string()),
    };

    // 1. Save memory entry to Qdrant
    store.save(&test_entry).await.expect("Qdrant save failed");

    // 🔥 Wait for Qdrant to index the point
    sleep(Duration::from_millis(500)).await;

    // 2. Semantic search: should return the test entry at top
    let results = store
        .semantic_search(
            &test_entry.session_id,
            &test_entry.embedding.as_ref().unwrap(),
            5,
        )
        .await
        .expect("Qdrant semantic_search failed");

    println!("Results returned from semantic_search: {:?}", results);

    // 3. Assert that recall finds our test entry
    assert!(!results.is_empty(), "Qdrant search returned no results!");

    let found = results.iter().any(|mem| mem.content == test_entry.content);
    assert!(found, "Test entry not found in Qdrant semantic recall results!");

    println!("✅ Qdrant Sprint 1 integration test PASSED!");
}
