// tests/test_chat_persistence.rs

mod test_helpers;

use mira_backend::memory::traits::MemoryStore;
use mira_backend::memory::types::MemoryEntry;
use chrono::Utc;

#[tokio::test]
async fn test_message_persistence() {
    println!("🧪 Testing message persistence...");
    
    let state = test_helpers::create_test_app_state().await;
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
        tags: Some(vec!["response".to_string(), "acknowledgment".to_string()]),
        summary: Some("Acknowledged wife's name".to_string()),
        memory_type: Some(mira_backend::memory::types::MemoryType::Feeling),
        logprobs: None,
        moderation_flag: None,
        system_fingerprint: None,
    };
    
    // Save messages
    state.sqlite_store.save(&user_msg).await
        .expect("Failed to save user message");
    state.sqlite_store.save(&assistant_msg).await
        .expect("Failed to save assistant message");
    
    println!("✅ Messages saved to SQLite");
    
    // Verify persistence by loading recent messages
    let recent = state.sqlite_store.load_recent(session_id, 10).await
        .expect("Failed to load recent messages");
    
    assert_eq!(recent.len(), 2, "Should have 2 messages");
    
    // Messages come back in reverse chronological order
    assert_eq!(recent[0].role, "assistant");
    assert_eq!(recent[1].role, "user");
    
    // Verify content
    assert!(recent[1].content.contains("Sarah"));
    assert!(recent[0].content.contains("beautiful name"));
    
    println!("✅ Messages successfully persisted and retrieved");
    println!("   - User: {}", recent[1].content);
    println!("   - Assistant: {}", recent[0].content);
}
