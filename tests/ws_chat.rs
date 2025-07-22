// tests/ws_chat.rs

use tokio_tungstenite::connect_async;
use serde_json::json;
use futures::{SinkExt, StreamExt};

#[tokio::test]
async fn ws_connects_and_accepts_message() {
    let url = "ws://localhost:8080/ws/chat";
    let (mut ws, _resp) = connect_async(url).await.expect("WS connect failed");

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

#[tokio::test]
async fn ws_rejects_malformed_input() {
    let url = "ws://localhost:8080/ws/chat";
    let (mut ws, _resp) = connect_async(url).await.expect("WS connect failed");

    ws.send(tokio_tungstenite::tungstenite::Message::Text("not json".to_string().into())).await.unwrap();

    if let Some(Ok(msg)) = ws.next().await {
        match msg {
            tokio_tungstenite::tungstenite::Message::Text(text) => {
                let v: serde_json::Value = serde_json::from_str(&text).unwrap();
                assert_eq!(v["type"], "error");
            }
            _ => panic!("Expected text error response"),
        }
    }
}

#[tokio::test]
async fn ws_handles_typing_indicator() {
    let url = "ws://localhost:8080/ws/chat";
    let (mut ws, _resp) = connect_async(url).await.expect("WS connect failed");

    let typing_msg = serde_json::json!({
        "type": "typing",
        "active": true
    }).to_string();

    ws.send(tokio_tungstenite::tungstenite::Message::Text(typing_msg.into())).await.unwrap();
    // Should not error, might not respond
}
