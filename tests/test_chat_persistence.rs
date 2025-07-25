// tests/test_chat_persistence.rs

use mira_backend::memory::sqlite::store::SqliteMemoryStore;
use mira_backend::memory::qdrant::store::QdrantMemoryStore;
use mira_backend::memory::traits::MemoryStore;
use mira_backend::memory::types::MemoryEntry;
use mira_backend::handlers::AppState;
use mira_backend::llm::OpenAIClient;
use sqlx::SqlitePool;
use std::sync::Arc;
use reqwest::Client;
use chrono::Utc;

/// Helper function to create test app state
async fn create_test_state() -> Arc<AppState> {
    // Use in-memory SQLite for tests
    let pool = SqlitePool::connect(":memory:").await
        .expect("Failed to create in-memory SQLite pool");
    
    // Run migrations
    mira_backend::memory::sqlite::migration::run_migrations(&pool).await
        .expect("Failed to run migrations");
    
    let sqlite_store = Arc::new(SqliteMemoryStore::new(pool));
    
    // For tests, we'll use the real Qdrant if available, or skip those parts
    let qdrant_url = std::env::var("QDRANT_URL")
        .unwrap_or_else(|_| "http://localhost:6333".to_string());
    let qdrant_collection = "mira-test-memory".to_string();
    
    let qdrant_store = Arc::new(QdrantMemoryStore::new(
        Client::new(),
        qdrant_url,
        qdrant_collection,
    ));
    
    let llm_client = Arc::new(OpenAIClient::new());
    
    Arc::new(AppState {
        sqlite_store,
        qdrant_store,
        llm_client,
    })
}

#[tokio::test]
async fn test_message_persistence() {
    println!("🧪 Testing message persistence...");
    
    let state = create_test_state().await;
    let session_id = "test-persistence";
    
    // Create test messages
    let user_msg = MemoryEntry {
        id: None,
        session_id: session_id.to_string(),
        role: "user".to_string(),
        content: "My wife's name is Sarah".to_string(),
        timestamp: Utc::now(),
        embedding: None, // Skip embeddings for basic test
        salience: Some(8.0),
        tags: Some(vec!["personal".to_string(), "family".to_string()]),
        summary: Some("User mentioned wife's name".to_string()),
        memory_type: Some(mira_backend::memory::types::MemoryType::Fact),
        logprobs: None,
        moderation_flag: None,
        system_fingerprint: None,
    };
    
    let assistant_msg = MemoryEntry {
        id: None,
        session_id: session_id.to_string(),
        role: "assistant".to_string(),
        content: "Sarah - that's a beautiful name! Tell me more about her.".to_string(),
        timestamp: Utc::now(),
        embedding: None,
        salience: Some(7.0),
        tags: Some(vec!["warm".to_string()]),
        summary: Some("Acknowledged wife's name".to_string()),
        memory_type: Some(mira_backend::memory::types::MemoryType::Other),
        logprobs: None,
        moderation_flag: None,
        system_fingerprint: None,
    };
    
    // Save messages
    println!("💾 Saving user message...");
    state.sqlite_store.save(&user_msg).await
        .expect("Failed to save user message");
    
    println!("💾 Saving assistant message...");
    state.sqlite_store.save(&assistant_msg).await
        .expect("Failed to save assistant message");
    
    // Load recent messages
    println!("📚 Loading recent messages...");
    let recent = state.sqlite_store.load_recent(session_id, 10).await
        .expect("Failed to load recent messages");
    
    // Verify messages were saved
    assert_eq!(recent.len(), 2, "Should have 2 messages");
    assert_eq!(recent[0].role, "assistant", "Most recent should be assistant");
    assert_eq!(recent[1].role, "user", "Second should be user");
    assert!(recent[1].content.contains("Sarah"), "User message should contain wife's name");
    
    println!("✅ Messages persisted and loaded correctly!");
}

#[tokio::test]
async fn test_chat_history_api() {
    println!("🧪 Testing chat history API functionality...");
    
    let state = create_test_state().await;
    let session_id = "peter-eternal"; // Use the hardcoded session from handlers
    
    // Save some test messages with different timestamps
    for i in 0..5 {
        let msg = MemoryEntry {
            id: None,
            session_id: session_id.to_string(),
            role: if i % 2 == 0 { "user".to_string() } else { "assistant".to_string() },
            content: format!("Test message {}", i),
            timestamp: Utc::now() - chrono::Duration::minutes(5 - i as i64),
            embedding: None,
            salience: Some(5.0),
            tags: Some(vec!["test".to_string()]),
            summary: None,
            memory_type: None,
            logprobs: None,
            moderation_flag: None,
            system_fingerprint: None,
        };
        
        state.sqlite_store.save(&msg).await
            .expect("Failed to save test message");
    }
    
    // Test loading with pagination
    println!("📖 Testing pagination...");
    
    // Load first page
    let query = r#"
        SELECT id, session_id, role, content, timestamp, embedding, salience, tags, summary, memory_type,
               logprobs, moderation_flag, system_fingerprint
        FROM chat_history
        WHERE session_id = ?
        ORDER BY timestamp DESC
        LIMIT ? OFFSET ?
    "#;
    
    let rows = sqlx::query(query)
        .bind(session_id)
        .bind(3i64) // limit
        .bind(0i64) // offset
        .fetch_all(&state.sqlite_store.pool)
        .await
        .expect("Failed to query history");
    
    assert_eq!(rows.len(), 3, "Should get 3 messages on first page");
    
    // Load second page
    let rows2 = sqlx::query(query)
        .bind(session_id)
        .bind(3i64) // limit
        .bind(3i64) // offset
        .fetch_all(&state.sqlite_store.pool)
        .await
        .expect("Failed to query second page");
    
    assert_eq!(rows2.len(), 2, "Should get 2 messages on second page");
    
    println!("✅ Chat history API working correctly!");
}

#[tokio::test]
async fn test_memory_recall_context() {
    println!("🧪 Testing memory recall context building...");
    
    let state = create_test_state().await;
    let session_id = "test-recall";
    
    // Create messages with specific content for testing recall
    let important_fact = MemoryEntry {
        id: None,
        session_id: session_id.to_string(),
        role: "user".to_string(),
        content: "I'm allergic to peanuts - this is very important!".to_string(),
        timestamp: Utc::now() - chrono::Duration::hours(2),
        embedding: None,
        salience: Some(10.0), // High salience
        tags: Some(vec!["health".to_string(), "critical".to_string()]),
        summary: Some("User has peanut allergy".to_string()),
        memory_type: Some(mira_backend::memory::types::MemoryType::Fact),
        logprobs: None,
        moderation_flag: None,
        system_fingerprint: None,
    };
    
    state.sqlite_store.save(&important_fact).await
        .expect("Failed to save important fact");
    
    // Add some filler messages
    for i in 0..20 {
        let filler = MemoryEntry {
            id: None,
            session_id: session_id.to_string(),
            role: if i % 2 == 0 { "user".to_string() } else { "assistant".to_string() },
            content: format!("Casual conversation message {}", i),
            timestamp: Utc::now() - chrono::Duration::minutes(60 - i as i64),
            embedding: None,
            salience: Some(3.0),
            tags: Some(vec!["chat".to_string()]),
            summary: None,
            memory_type: None,
            logprobs: None,
            moderation_flag: None,
            system_fingerprint: None,
        };
        state.sqlite_store.save(&filler).await.ok();
    }
    
    // Test recall with increased context window
    let recent = state.sqlite_store.load_recent(session_id, 30).await
        .expect("Failed to load recent with larger window");
    
    // The important fact should be in the history
    let has_allergy_info = recent.iter()
        .any(|msg| msg.content.contains("allergic to peanuts"));
    
    assert!(has_allergy_info, "Important health information should be in recall context");
    println!("✅ Memory recall context includes important historical information!");
}

#[tokio::test] 
#[ignore] // This test requires OpenAI API key
async fn test_embedding_generation() {
    println!("🧪 Testing embedding generation...");
    
    dotenv::dotenv().ok(); // Load .env file
    
    if std::env::var("OPENAI_API_KEY").is_err() {
        println!("⚠️  Skipping embedding test - no OPENAI_API_KEY set");
        return;
    }
    
    let state = create_test_state().await;
    
    let test_text = "This is a test message for embedding generation";
    match state.llm_client.get_embedding(test_text).await {
        Ok(embedding) => {
            assert_eq!(embedding.len(), 3072, "Embedding should be 3072 dimensions");
            println!("✅ Embedding generated successfully!");
        }
        Err(e) => {
            panic!("Failed to generate embedding: {:?}", e);
        }
    }
}

// Helper function to print recent messages (for debugging)
#[allow(dead_code)]
async fn debug_print_messages(state: &AppState, session_id: &str) {
    println!("\n📋 Recent messages for session '{}':", session_id);
    let recent = state.sqlite_store.load_recent(session_id, 10).await
        .expect("Failed to load messages");
    
    for (i, msg) in recent.iter().enumerate() {
        println!("  {}. [{}] {}: {}", 
            i + 1,
            msg.role,
            msg.timestamp.format("%H:%M:%S"),
            msg.content.chars().take(50).collect::<String>()
        );
    }
    println!();
}
