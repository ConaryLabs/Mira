// tests/rest_chat.rs

use reqwest::Client;
use serde_json::json;

#[tokio::test]
async fn rest_chat_works() {
    let client = Client::new();
    let payload = json!({
        "message": "What's your favorite color?",
        "persona_override": null
    });
    let resp = client.post("http://localhost:8080/chat")
        .json(&payload)
        .send().await.expect("Failed to POST /chat");

    assert!(resp.status().is_success());
    let reply: serde_json::Value = resp.json().await.unwrap();
    assert!(reply["output"].is_string());
}

#[tokio::test]
async fn rest_chat_handles_bad_json() {
    let client = Client::new();
    let resp = client.post("http://localhost:8080/chat")
        .body("{not:json")
        .header("Content-Type", "application/json")
        .send().await.expect("POST should not crash");

    assert!(resp.status().is_client_error());
}
