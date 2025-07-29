// tests/sprint2_features.rs

use serde_json::json;
use tokio_tungstenite::connect_async;
use futures::{StreamExt, SinkExt};

#[tokio::test]
#[ignore] // Requires running server with full WebSocket implementation
async fn test_persona_switching_websocket() {
    let url = "ws://localhost:8080/ws/chat";
    let (mut ws, _) = connect_async(url).await.expect("Failed to connect");
    
    // Send initial message with Default persona
    let msg = json!({
        "type": "message",
        "content": "Hey Mira, how are you?",
        "persona": "Default"
    });
    ws.send(tokio_tungstenite::tungstenite::Message::Text(msg.to_string().into())).await.unwrap();
    
    // Get some response chunks to establish baseline
    let mut first_mood = None;
    for _ in 0..5 {
        if let Some(Ok(msg)) = ws.next().await {
            match msg {
                tokio_tungstenite::tungstenite::Message::Text(text) => {
                    let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
                    if parsed["type"] == "chunk" && first_mood.is_none() {
                        first_mood = parsed["mood"].as_str().map(String::from);
                    }
                }
                _ => {}
            }
        }
    }
    
    // Wait for response to complete
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    
    // Switch persona silently (no notification expected)
    let switch_msg = json!({
        "type": "switch_persona",
        "persona": "Forbidden",
        "smooth_transition": true
    });
    ws.send(tokio_tungstenite::tungstenite::Message::Text(switch_msg.to_string().into())).await.unwrap();
    
    // Small delay to let the switch process
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    
    // Send another message - should use new persona internally
    let msg2 = json!({
        "type": "message",
        "content": "Tell me something spicy",
        "persona": null  // Let it use the switched persona
    });
    ws.send(tokio_tungstenite::tungstenite::Message::Text(msg2.to_string().into())).await.unwrap();
    
    // The response should subtly reflect the Forbidden persona
    // but we won't check for explicit persona_update messages
    let mut got_response = false;
    for _ in 0..10 {
        if let Some(Ok(msg)) = ws.next().await {
            match msg {
                tokio_tungstenite::tungstenite::Message::Text(text) => {
                    let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
                    // Just verify we get chunks, not looking for persona_update
                    if parsed["type"] == "chunk" || parsed["type"] == "aside" {
                        got_response = true;
                        // The mood might be different, but that's organic
                        println!("Response after switch: {:?}", parsed);
                    }
                }
                _ => {}
            }
        }
    }
    
    assert!(got_response, "Should receive response after persona switch");
    // NOT asserting any persona_update message - that's the point!
}
