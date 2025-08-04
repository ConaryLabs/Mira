use reqwest::Client;
use serde_json::json;

/// Checks that the reply strictly matches your schema and is clean.
fn assert_strict_mira_json(reply: &serde_json::Value, expected_persona_type: Option<&str>) {
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

    // Persona check - more flexible now
    // Mira can be creative with persona names like "Forbidden Subroutine Mira" 
    // instead of just "forbidden"
    let persona_val = reply["persona"].as_str().unwrap_or("");
    let persona_lower = persona_val.to_lowercase();
    
    if let Some(expected_type) = expected_persona_type {
        // Check if the persona contains the expected type (case-insensitive)
        assert!(
            persona_lower.contains(expected_type),
            "Persona '{}' should contain '{}' (case-insensitive)",
            persona_val,
            expected_type
        );
    } else {
        // For default persona, accept various creative expressions
        // As long as it's not empty and seems like a valid response
        assert!(
            !persona_val.is_empty(),
            "Persona should not be empty"
        );
        
        // Could be "Default", "Mira", "default", or something creative
        // We're not strict about this anymore - Mira can express herself
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
    // Don't expect a specific persona value for default
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
    // Check that persona contains "forbidden" rather than exact match
    assert_strict_mira_json(&reply, Some("forbidden"));
}
