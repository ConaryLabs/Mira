// tests/memory_service_test.rs

use mira_backend::config::CONFIG;
use mira_backend::llm::client::OpenAIClient;
use mira_backend::memory::sqlite::store::SqliteMemoryStore;
use mira_backend::llm::chat_service::ChatResponse;
use mira_backend::memory::MemoryService;
use sqlx::sqlite::SqlitePoolOptions;
use std::sync::Arc;
use uuid::Uuid;

/// Helper function to set up a clean, isolated test environment.
async fn setup_test_environment() -> (MemoryService, String) {
    // 1. Create a connection pool to a new in-memory SQLite database.
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("Failed to create in-memory SQLite pool.");
    
    // 2. Create the store and run migrations on it.
    let store = SqliteMemoryStore::new(pool);
    store.run_migrations().await.unwrap();
    let sqlite_store = Arc::new(store);

    // 3. Configure a connection to a TEST Qdrant instance.
    let qdrant_store =
        Arc::new(mira_backend::memory::qdrant::store::QdrantMemoryStore::new(&CONFIG.qdrant_test_url, &CONFIG.qdrant_test_collection)
            .await
            .unwrap());

    // 4. Set up the LLM Client.
    let llm_client = OpenAIClient::new().unwrap();

    // 5. Create the MemoryService instance.
    let memory_service = MemoryService::new(
        llm_client,
        sqlite_store,
        qdrant_store,
    )
    .await
    .unwrap();
    
    // Generate a unique session ID for this specific test run.
    let session_id = format!("test_session_{}", Uuid::new_v4());

    (memory_service, session_id)
}

#[tokio::test]
async fn test_save_and_retrieve_user_message() {
    // ARRANGE
    let (memory_service, session_id) = setup_test_environment().await;
    let content = "This is a test message from a user.";

    // ACT
    let save_result = memory_service
        .save_user_message(&session_id, content, None)
        .await;
    
    // ASSERT
    assert!(save_result.is_ok());

    let messages = memory_service
        .get_recent_context(&session_id, 10)
        .await
        .unwrap();

    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].content, content);
    assert_eq!(messages[0].role, "user");
}

#[tokio::test]
async fn test_save_and_retrieve_assistant_message() {
    // ARRANGE
    let (memory_service, session_id) = setup_test_environment().await;
    
    // THE FIX: Provide all required fields for the ChatResponse struct.
    let response = ChatResponse {
        output: "This is a test response from the assistant.".to_string(),
        persona: "test_persona".to_string(),
        mood: "test_mood".to_string(),
        salience: 8, // This is a usize
        summary: "Assistant test response".to_string(),
        memory_type: "Fact".to_string(),
        tags: vec!["test".to_string()],
        intent: None,
        monologue: None,
        reasoning_summary: None,
    };

    // ACT
    let save_result = memory_service
        .save_assistant_response(&session_id, &response)
        .await;
    
    // ASSERT
    assert!(save_result.is_ok());

    let messages = memory_service
        .get_recent_context(&session_id, 10)
        .await
        .unwrap();

    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].content, response.output);
    assert_eq!(messages[0].role, "assistant");
    assert_eq!(messages[0].salience, Some(response.salience as f32));
}
