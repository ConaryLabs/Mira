// tests/phase2_multi_head_test.rs
// PHASE 2: Integration test for multi-head embeddings and chunking.
// UPDATED: Made test helpers more robust to handle race conditions.

use anyhow::Result;
use mira_backend::{
    config::CONFIG,
    llm::client::OpenAIClient,
    memory::{
        qdrant::{multi_store::QdrantMultiStore, store::QdrantMemoryStore},
        sqlite::{migration::run_migrations, store::SqliteMemoryStore},
    },
    services::memory::MemoryService,
};
use serde_json::json;
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::Mutex;

// A mutex to ensure that tests modifying the environment are run serially.
static ENV_MUTEX: Mutex<()> = Mutex::const_new(());

/// Helper to create a MemoryService instance configured for testing.
async fn setup_test_memory_service() -> Result<MemoryService> {
    // Ensure we're using the test Qdrant instance
    let qdrant_url = &CONFIG.qdrant_test_url;
    let collection_base_name = &CONFIG.qdrant_test_collection;

    // The single store for backward compatibility
    let qdrant_store = Arc::new(QdrantMemoryStore::new(qdrant_url, "mira-test-legacy").await?);
    
    // The multi-store for robust memory, now correctly initialized with the test collection name
    let multi_store = Arc::new(QdrantMultiStore::new(qdrant_url, collection_base_name).await?);

    // Correctly create an in-memory SQLite pool for the test
    let pool = SqlitePool::connect("sqlite::memory:").await?;
    run_migrations(&pool).await?; 
    let sqlite_store = Arc::new(SqliteMemoryStore::new(pool));

    let llm_client = OpenAIClient::new()?;

    let memory_service = MemoryService::new_with_multi_store(
        sqlite_store,
        qdrant_store,
        multi_store,
        llm_client,
    );

    Ok(memory_service)
}

/// Helper to clear all points from a Qdrant collection.
async fn clear_qdrant_collection(collection_name: &str) -> Result<()> {
    let client = reqwest::Client::builder().http1_only().build()?;
    let url = format!(
        "{}/collections/{}/points/delete",
        CONFIG.qdrant_test_url, collection_name
    );
    let response = client
        .post(&url)
        .json(&json!({
            "filter": {}
        }))
        .send()
        .await?;

    // If the collection doesn't exist (404), that's fine for a clear operation.
    if response.status().is_success() || response.status().as_u16() == 404 {
        Ok(())
    } else {
        let error_body = response.text().await?;
        anyhow::bail!("Failed to clear Qdrant collection '{}': {}", collection_name, error_body);
    }
}

/// Helper to get all points from a Qdrant collection.
async fn get_all_points(collection_name: &str) -> Result<serde_json::Value> {
    let client = reqwest::Client::builder().http1_only().build()?;
    let url = format!(
        "{}/collections/{}/points/scroll",
        CONFIG.qdrant_test_url, collection_name
    );
    let response = client.post(&url).json(&json!({"limit": 100})).send().await?;

    if !response.status().is_success() {
        let error_body = response.text().await?;
        anyhow::bail!("Failed to scroll Qdrant collection '{}': {}", collection_name, error_body);
    }
    
    Ok(response.json().await?)
}

#[tokio::test]
#[ignore] // This test requires a running Qdrant instance and OpenAI API key.
async fn test_multi_head_embedding_and_chunking() -> Result<()> {
    let _guard = ENV_MUTEX.lock().await;

    unsafe {
        std::env::set_var("MIRA_AGGRESSIVE_METADATA_ENABLED", "true");
    }
    let memory_service = setup_test_memory_service().await?;
    
    let base_collection = &CONFIG.qdrant_test_collection;
    clear_qdrant_collection(&format!("{}-semantic", base_collection)).await?;
    clear_qdrant_collection(&format!("{}-code", base_collection)).await?;
    clear_qdrant_collection(&format!("{}-summary", base_collection)).await?;

    let session_id = "phase2-test-session";
    let user_message = r#"
        That's a great point about performance. Let's refactor this Rust function to be more efficient.
        
        ```rust
        fn calculate_sum(numbers: Vec<i32>) -> i32 {
            let mut sum = 0;
            for num in numbers {
                sum += num;
            }
            sum
        }
        ```

        I think we can use the `iter().sum()` method instead to make it more idiomatic and potentially faster.
        What are your thoughts on this approach?
    "#;

    memory_service.save_user_message(session_id, user_message, None).await?;

    // A small delay to allow Qdrant to process the writes before we query.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // 1. Check the SEMANTIC collection
    let semantic_collection_name = format!("{}-semantic", base_collection);
    let semantic_points = get_all_points(&semantic_collection_name).await?;
    let semantic_chunks = semantic_points["result"]["points"].as_array().unwrap();

    assert!(!semantic_chunks.is_empty(), "Semantic collection should have chunks.");
    assert!(
        semantic_chunks[0]["payload"]["content"].as_str().unwrap().contains("refactor this Rust function"),
        "Semantic chunk content seems incorrect."
    );
    assert!(
        semantic_chunks[0]["payload"]["tags"].to_string().contains("head:semantic"),
        "Semantic chunk should be tagged with the correct head."
    );

    // 2. Check the CODE collection
    let code_collection_name = format!("{}-code", base_collection);
    let code_points = get_all_points(&code_collection_name).await?;
    let code_chunks = code_points["result"]["points"].as_array().unwrap();

    assert!(!code_chunks.is_empty(), "Code collection should have chunks.");
    assert!(
        code_chunks[0]["payload"]["content"].as_str().unwrap().contains("fn calculate_sum"),
        "Code chunk content seems incorrect."
    );
    assert!(
        code_chunks[0]["payload"]["tags"].to_string().contains("head:code"),
        "Code chunk should be tagged with the correct head."
    );

    // 3. Check the SUMMARY collection (should be empty for a user message)
    let summary_collection_name = format!("{}-summary", base_collection);
    let summary_points = get_all_points(&summary_collection_name).await?;
    let summary_chunks = summary_points["result"]["points"].as_array().unwrap();

    assert!(summary_chunks.is_empty(), "Summary collection should be empty for a user message.");

    unsafe {
        std::env::remove_var("MIRA_AGGRESSIVE_METADATA_ENABLED");
    }

    Ok(())
}

