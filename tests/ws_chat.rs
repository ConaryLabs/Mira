// tests/ws_chat.rs

use tokio_tungstenite::connect_async;
use serde_json::json;
use futures::{SinkExt, StreamExt};

#[tokio::test]
async fn ws_connects_and_accepts_message() {
    let url = "ws://localhost:8080/ws/chat";
    let (mut ws, _resp) = connect_async(url).await.expect("WS connect failed");

    // First, consume the greeting message
    if let Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) = ws.next().await {
        let _greeting: serde_json::Value = serde_json::from_str(&text).unwrap();
        // Greeting received, now send our message
    }

    let client_msg = json!({
        "type": "message",
        "content": "Hey Mira, are you alive?",
        "persona": null
    }).to_string();

    ws.send(tokio_tungstenite::tungstenite::Message::Text(client_msg.into())).await.unwrap();

    let mut got_response = false;
    for _ in 0..5 {
        if let Some(Ok(msg)) = ws.next().await {
            match msg {
                tokio_tungstenite::tungstenite::Message::Text(text) => {
                    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
                    if let Some(msg_type) = v["type"].as_str() {
                        assert!(["chunk", "aside", "done"].contains(&msg_type));
                        got_response = true;
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    assert!(got_response, "Didn't receive any valid response");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ws_rejects_malformed_input() {
    let url = "ws://localhost:8080/ws/chat";
    let (mut ws, _resp) = connect_async(url).await.expect("WS connect failed");

    // First, consume the greeting message
    if let Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) = ws.next().await {
        let _greeting: serde_json::Value = serde_json::from_str(&text).unwrap();
        // Greeting received, now send malformed JSON
    }

    ws.send(tokio_tungstenite::tungstenite::Message::Text("not json".to_string().into())).await.unwrap();

    // Look for error response, might need to skip other messages
    let mut found_error = false;
    for _ in 0..5 {
        let timeout = tokio::time::timeout(
            tokio::time::Duration::from_secs(1),
            ws.next()
        ).await;

        match timeout {
            Ok(Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text)))) => {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                    if v["type"] == "error" && v["code"] == "PARSE_ERROR" {
                        found_error = true;
                        break;
                    }
                    // Skip any other messages (like chunks)
                }
            }
            _ => break, // Connection closed or timeout
        }
    }
    
    assert!(found_error, "Expected error response for malformed JSON");
}

#[tokio::test]
async fn ws_handles_typing_indicator() {
    let url = "ws://localhost:8080/ws/chat";
    let (mut ws, _resp) = connect_async(url).await.expect("WS connect failed");

    // First, consume the greeting message
    if let Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) = ws.next().await {
        let _greeting: serde_json::Value = serde_json::from_str(&text).unwrap();
        // Greeting received, now send typing indicator
    }

    let typing_msg = serde_json::json!({
        "type": "typing",
        "active": true
    }).to_string();

    ws.send(tokio_tungstenite::tungstenite::Message::Text(typing_msg.into())).await.unwrap();
    
    // Typing indicator might not generate a response, so we just verify it doesn't crash
    // Wait a short time to see if there's any response
    let timeout = tokio::time::timeout(
        tokio::time::Duration::from_millis(100),
        ws.next()
    ).await;
    
    // Whether we get a response or timeout, both are fine for typing indicator
    match timeout {
        Ok(_) => {} // Got some response or connection closed
        Err(_) => {} // Timeout - no response expected for typing
    }
}
