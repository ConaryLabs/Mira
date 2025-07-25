// tests/debug_database.rs
// Run with: cargo test debug_database -- --nocapture

use sqlx::{SqlitePool, Row};

#[tokio::test]
async fn debug_database() {
    println!("\n🔍 DATABASE DEBUG TOOL\n");
    
    // Connect to the actual database file
    let pool = SqlitePool::connect("sqlite://mira.db").await
        .expect("Failed to connect to mira.db");
    
    // 1. Check total message count
    let count_result = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM chat_history WHERE session_id = 'peter-eternal'"
    )
    .fetch_one(&pool)
    .await;
    
    match count_result {
        Ok(count) => println!("📊 Total messages for 'peter-eternal': {}", count),
        Err(e) => println!("❌ Error counting messages: {}", e),
    }
    
    // 2. Check messages from last hour
    let recent_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM chat_history 
         WHERE session_id = 'peter-eternal' 
         AND timestamp > datetime('now', '-1 hour')"
    )
    .fetch_one(&pool)
    .await
    .unwrap_or(0);
    
    println!("⏰ Messages in last hour: {}", recent_count);
    
    // 3. Show last 10 messages
    println!("\n📜 Last 10 messages:");
    println!("{:-<80}", "");
    
    let messages = sqlx::query(
        r#"
        SELECT id, role, content, timestamp, salience, tags
        FROM chat_history
        WHERE session_id = 'peter-eternal'
        ORDER BY timestamp DESC
        LIMIT 10
        "#
    )
    .fetch_all(&pool)
    .await
    .expect("Failed to fetch messages");
    
    for (i, row) in messages.iter().enumerate() {
        let _id: i64 = row.get("id");
        let role: String = row.get("role");
        let content: String = row.get("content");
        let timestamp: String = row.get("timestamp");
        let salience: Option<f32> = row.get("salience");
        let tags: Option<String> = row.get("tags");
        
        println!("\n{}. [{}] @ {}", 
            i + 1,
            role,
            timestamp
        );
        println!("   Content: {}", 
            content.chars().take(100).collect::<String>()
        );
        if let Some(tags) = tags {
            println!("   Tags: {}", tags);
        }
        if let Some(salience) = salience {
            println!("   Salience: {}", salience);
        }
    }
    
    // 4. Check for any errors or issues
    println!("\n🔧 Database integrity check:");
    let integrity = sqlx::query_scalar::<_, String>("PRAGMA integrity_check")
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|_| "ERROR".to_string());
    
    println!("   Integrity: {}", integrity);
    
    // 5. Check if embeddings are being saved
    let embedding_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM chat_history 
         WHERE session_id = 'peter-eternal' 
         AND embedding IS NOT NULL"
    )
    .fetch_one(&pool)
    .await
    .unwrap_or(0);
    
    println!("   Messages with embeddings: {}", embedding_count);
    
    println!("\n{:-<80}", "");
    println!("✅ Debug complete!\n");
}
