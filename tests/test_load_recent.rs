// tests/test_load_recent.rs

use mira_backend::memory::sqlite::store::SqliteMemoryStore;
use mira_backend::memory::traits::MemoryStore;
use sqlx::SqlitePool;
use std::sync::Arc;

#[tokio::test]
async fn test_load_recent_messages() {
    println!("\n🔍 TESTING LOAD_RECENT FUNCTION\n");
    
    // Connect to the actual database
    let pool = SqlitePool::connect("sqlite://mira.db").await
        .expect("Failed to connect to mira.db");
    
    let store = Arc::new(SqliteMemoryStore::new(pool));
    
    // Test load_recent with different counts
    for count in [10, 20, 30] {
        println!("\n📚 Loading {} recent messages:", count);
        let messages = store.load_recent("peter-eternal", count).await
            .expect("Failed to load messages");
        
        println!("   Loaded: {} messages", messages.len());
        
        // Show first and last message
        if let Some(first) = messages.first() {
            println!("   First (newest): [{}] {} - {}",
                first.role,
                first.timestamp.format("%Y-%m-%d %H:%M:%S"),
                first.content.chars().take(30).collect::<String>()
            );
        }
        
        if let Some(last) = messages.last() {
            println!("   Last (oldest): [{}] {} - {}",
                last.role,
                last.timestamp.format("%Y-%m-%d %H:%M:%S"),
                last.content.chars().take(30).collect::<String>()
            );
        }
        
        // Check for "Courtney" mentions
        let courtney_mentions = messages.iter()
            .filter(|m| m.content.to_lowercase().contains("courtney"))
            .count();
        println!("   Messages mentioning 'Courtney': {}", courtney_mentions);
    }
}
