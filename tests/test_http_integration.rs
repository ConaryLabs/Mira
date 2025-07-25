// tests/test_http_integration.rs

use axum::http::StatusCode;
use serde_json::json;

#[tokio::test]
#[ignore] // Run with: cargo test -- --ignored
async fn test_chat_endpoint_integration() {
    println!("🧪 Testing chat endpoint integration...");
    
    // This test assumes the server is running on localhost:8080
    let client = reqwest::Client::new();
    
    // Test sending a message
    let response = client
        .post("http://localhost:8080/chat")
        .json(&json!({
            "message": "Hello, this is a test message!",
            "persona_override": null
        }))
        .send()
        .await;
    
    match response {
        Ok(resp) => {
            assert_eq!(resp.status(), StatusCode::OK, "Chat endpoint should return 200");
            let body: serde_json::Value = resp.json().await.unwrap();
            println!("📨 Response: {}", serde_json::to_string_pretty(&body).unwrap());
            
            assert!(body.get("output").is_some(), "Response should have output field");
            assert!(body.get("mood").is_some(), "Response should have mood field");
        }
        Err(e) => {
            println!("⚠️  Server not running? Error: {}", e);
            println!("   Run the server first with: cargo run");
        }
    }
}

#[tokio::test]
#[ignore] // Run with: cargo test -- --ignored
async fn test_history_endpoint_integration() {
    println!("🧪 Testing history endpoint integration...");
    
    let client = reqwest::Client::new();
    
    // First, send a test message
    let _ = client
        .post("http://localhost:8080/chat")
        .json(&json!({
            "message": "Test message for history",
        }))
        .send()
        .await;
    
    // Give it a moment to save
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    
    // Now fetch history
    let response = client
        .get("http://localhost:8080/chat/history?limit=10")
        .send()
        .await;
    
    match response {
        Ok(resp) => {
            assert_eq!(resp.status(), StatusCode::OK, "History endpoint should return 200");
            let body: serde_json::Value = resp.json().await.unwrap();
            println!("📜 History: {}", serde_json::to_string_pretty(&body).unwrap());
            
            assert!(body.get("messages").is_some(), "Response should have messages array");
            assert!(body.get("session_id").is_some(), "Response should have session_id");
            
            if let Some(messages) = body.get("messages").and_then(|m| m.as_array()) {
                println!("   Found {} messages in history", messages.len());
            }
        }
        Err(e) => {
            println!("⚠️  Server not running? Error: {}", e);
        }
    }
}
