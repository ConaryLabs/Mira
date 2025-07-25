// tests/test_direct_save.rs

use mira_backend::memory::sqlite::store::SqliteMemoryStore;
use mira_backend::memory::traits::MemoryStore;
use mira_backend::memory::types::MemoryEntry;
use sqlx::SqlitePool;
use chrono::Utc;
use std::sync::Arc;

#[tokio::test]
async fn test_direct_database_save() {
    println!("\n🧪 DIRECT DATABASE SAVE TEST\n");
    
    // Connect to the actual database
    let pool = SqlitePool::connect("sqlite://mira.db").await
        .expect("Failed to connect to mira.db");
    
    let store = Arc::new(SqliteMemoryStore::new(pool.clone()));
    
    // Create a test message without embedding
    let test_msg = MemoryEntry {
        id: None,
        session_id: "peter-eternal".to_string(),
        role: "user".to_string(),
        content: format!("TEST MESSAGE: Direct save test at {}", Utc::now()),
        timestamp: Utc::now(),
        embedding: None, // No embedding!
        salience: Some(5.0),
        tags: Some(vec!["test".to_string(), "debug".to_string()]),
        summary: Some("Testing direct save without embeddings".to_string()),
        memory_type: None,
        logprobs: None,
        moderation_flag: None,
        system_fingerprint: None,
    };
    
    println!("💾 Attempting to save test message...");
    match store.save(&test_msg).await {
        Ok(_) => println!("✅ Message saved successfully!"),
        Err(e) => println!("❌ Failed to save: {:?}", e),
    }
    
    // Now check if it was saved
    println!("\n🔍 Checking if message was saved...");
    let recent = store.load_recent("peter-eternal", 5).await
        .expect("Failed to load recent messages");
    
    let found = recent.iter().any(|m| m.content.contains("TEST MESSAGE"));
    
    if found {
        println!("✅ Test message found in database!");
        
        // Count total messages now
        let count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM chat_history WHERE session_id = 'peter-eternal'"
        )
        .fetch_one(&pool)
        .await
        .unwrap_or(0);
        
        println!("📊 Total messages now: {}", count);
    } else {
        println!("❌ Test message NOT found in recent messages");
    }
    
    println!("\n✅ Test complete!\n");
}
