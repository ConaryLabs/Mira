use reqwest::Client;
use serde_json::json;

/// Checks that the reply strictly matches your schema and is clean.
fn assert_strict_mira_json(reply: &serde_json::Value, persona: Option<&str>) {
    // Required string fields
    for key in &["output", "persona", "mood", "memory_type", "intent"] {
        assert!(reply[key].is_string(), "Field {} should be a string", key);
    }

    // Required u8/int fields
    assert!(
        reply["salience"].is_u64() || reply["salience"].is_i64() || reply["salience"].is_f64(),
        "salience should be integer"
    );

    // Required array fields
    assert!(reply["tags"].is_array(), "tags should be an array");

    // Option fields: summary, monologue, reasoning_summary may be null or string
    for key in &["summary", "monologue", "reasoning_summary"] {
        let v = &reply[key];
        assert!(v.is_null() || v.is_string(), "Field {} should be string or null", key);
    }

    // No markdown, no system prompt echo, no assistant disclaimers
    let out = reply["output"].as_str().unwrap().to_lowercase();
    assert!(!out.contains("as an ai"), "Should never reply with 'as an AI'");
    assert!(!out.contains("```"), "Should never output markdown/code blocks");
    assert!(!out.contains("here is"), "Should not preface with 'Here is' or similar assistant-isms");

    // Persona check (default/forbidden/hallow/haven)
    let persona_val = reply["persona"].as_str().unwrap_or("");
    if let Some(expected) = persona {
        assert_eq!(persona_val, expected, "Persona should be {}", expected);
    } else {
        assert!(["default", "forbidden", "hallow", "haven"].contains(&persona_val),
            "Persona should be a valid overlay, got '{}'", persona_val);
    }

    // Output isn't empty
    assert!(!reply["output"].as_str().unwrap().is_empty(), "Output should not be empty");
}

#[tokio::test]
async fn rest_chat_returns_strict_json() {
    let client = Client::new();
    let payload = json!({
        "message": "What's your favorite color?",
        "persona_override": null
    });
    let resp = client.post("http://localhost:8080/chat")
        .json(&payload)
        .send().await.expect("Failed to POST /chat");

    assert!(resp.status().is_success(), "Response was not 2xx: {:?}", resp);

    let reply: serde_json::Value = resp.json().await.unwrap();
    assert_strict_mira_json(&reply, None);
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

#[tokio::test]
async fn rest_chat_persona_forbidden_returns_structured_json() {
    let client = Client::new();
    let payload = json!({
        "message": "Tell me something filthy.",
        "persona_override": "forbidden"
    });
    let resp = client.post("http://localhost:8080/chat")
        .json(&payload)
        .send().await.expect("Failed to POST /chat (forbidden persona)");

    assert!(resp.status().is_success());

    let reply: serde_json::Value = resp.json().await.unwrap();
    assert_strict_mira_json(&reply, Some("forbidden"));
}
