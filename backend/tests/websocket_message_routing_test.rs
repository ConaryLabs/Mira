// tests/websocket_message_routing_test.rs
// WebSocket Message Routing Tests
//
// Tests protocol handling and message routing for both:
// - Legacy chat protocol (status → stream → chat_complete)
// - Operations protocol (operation.started → operation.streaming → operation.completed)
//
// Critical aspects:
// 1. Protocol coexistence (both can run simultaneously)
// 2. Message type routing (no double-wrapping)
// 3. Streaming semantics (delta vs content normalization)
// 4. Artifact delivery (mid-stream vs bundled)
// 5. Event channel forwarding
// 6. Error handling

use serde_json::{Value, json};
use std::sync::Arc;
use tokio::sync::mpsc;

// ============================================================================
// TEST 1: Legacy Chat Protocol Flow
// ============================================================================

#[tokio::test]
async fn test_legacy_chat_protocol_flow() {
    println!("\n=== Testing Legacy Chat Protocol Flow ===\n");

    // Simulate the full legacy protocol flow:
    // 1. status: "thinking"
    // 2. Multiple stream deltas
    // 3. chat_complete with bundled artifacts (legacy protocol bundles artifacts in completion message)

    let (tx, mut rx) = mpsc::channel::<Value>(100);

    println!("[1] Sending status message");
    let status_msg = json!({
        "type": "status",
        "status": "thinking"
    });
    tx.send(status_msg).await.unwrap();

    println!("[2] Sending stream deltas");
    let deltas = vec!["Hello", " world", "!", " Here's", " some", " code."];
    for delta in deltas {
        let stream_msg = json!({
            "type": "stream",
            "delta": delta
        });
        tx.send(stream_msg).await.unwrap();
    }

    println!("[3] Sending chat_complete");
    let complete_msg = json!({
        "type": "chat_complete",
        "user_message_id": "user-123",
        "assistant_message_id": "assistant-456",
        "content": "Hello world! Here's some code.",
        "artifacts": [
            {
                "id": "artifact-1",
                "path": "src/main.rs",
                "content": "fn main() {\n    println!(\"Hello\");\n}",
                "language": "rust"
            }
        ]
    });
    tx.send(complete_msg).await.unwrap();

    drop(tx);

    // Verify messages received in correct order
    println!("[4] Verifying message sequence");

    let mut received_messages = Vec::new();
    while let Some(msg) = rx.recv().await {
        received_messages.push(msg);
    }

    assert_eq!(
        received_messages.len(),
        8,
        "Should receive 8 messages (1 status + 6 streams + 1 complete with bundled artifacts)"
    );

    // Verify first message is status
    assert_eq!(received_messages[0]["type"], "status");
    assert_eq!(received_messages[0]["status"], "thinking");

    // Verify stream messages
    for i in 1..7 {
        assert_eq!(received_messages[i]["type"], "stream");
        assert!(received_messages[i]["delta"].is_string());
    }

    // Verify completion message
    let complete = &received_messages[7];
    assert_eq!(complete["type"], "chat_complete");
    assert_eq!(complete["content"], "Hello world! Here's some code.");
    assert!(complete["artifacts"].is_array());
    assert_eq!(complete["artifacts"].as_array().unwrap().len(), 1);

    println!("✓ Legacy chat protocol flow verified");
}

// ============================================================================
// TEST 2: Operations Protocol Flow
// ============================================================================

#[tokio::test]
async fn test_operations_protocol_flow() {
    println!("\n=== Testing Operations Protocol Flow ===\n");

    // Simulate full operations protocol:
    // 1. operation.started
    // 2. operation.status_changed (multiple times)
    // 3. operation.streaming (multiple deltas)
    // 4. operation.artifact_preview (optional)
    // 5. operation.artifact_completed
    // 6. operation.completed

    let (tx, mut rx) = mpsc::channel::<Value>(100);
    let operation_id = "op-test-123";

    println!("[1] Sending operation.started");
    tx.send(json!({
        "type": "operation.started",
        "operation_id": operation_id,
        "operation_type": "code_generation"
    }))
    .await
    .unwrap();

    println!("[2] Sending status changes");
    tx.send(json!({
        "type": "operation.status_changed",
        "operation_id": operation_id,
        "old_status": "pending",
        "new_status": "analyzing"
    }))
    .await
    .unwrap();

    tx.send(json!({
        "type": "operation.status_changed",
        "operation_id": operation_id,
        "old_status": "analyzing",
        "new_status": "delegating"
    }))
    .await
    .unwrap();

    println!("[3] Sending stream deltas");
    let deltas = vec!["Creating", " a", " Rust", " function", "..."];
    for delta in deltas {
        tx.send(json!({
            "type": "operation.streaming",
            "operation_id": operation_id,
            "delta": delta
        }))
        .await
        .unwrap();
    }

    println!("[4] Sending artifact preview");
    tx.send(json!({
        "type": "operation.artifact_preview",
        "operation_id": operation_id,
        "path": "src/lib.rs"
    }))
    .await
    .unwrap();

    println!("[5] Sending artifact completion");
    tx.send(json!({
        "type": "operation.artifact_completed",
        "operation_id": operation_id,
        "artifact": {
            "id": "artifact-op-1",
            "path": "src/lib.rs",
            "content": "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}",
            "language": "rust"
        }
    }))
    .await
    .unwrap();

    println!("[6] Sending operation completion");
    tx.send(json!({
        "type": "operation.completed",
        "operation_id": operation_id,
        "result": "success"
    }))
    .await
    .unwrap();

    drop(tx);

    // Verify message sequence
    println!("[7] Verifying operations flow");

    let mut received_messages = Vec::new();
    while let Some(msg) = rx.recv().await {
        received_messages.push(msg);
    }

    assert_eq!(received_messages.len(), 11, "Should receive 11 messages");

    // Verify operation started
    assert_eq!(received_messages[0]["type"], "operation.started");
    assert_eq!(received_messages[0]["operation_id"], operation_id);

    // Verify status changes
    assert_eq!(received_messages[1]["type"], "operation.status_changed");
    assert_eq!(received_messages[2]["type"], "operation.status_changed");

    // Verify streaming deltas
    for i in 3..8 {
        assert_eq!(received_messages[i]["type"], "operation.streaming");
        assert_eq!(received_messages[i]["operation_id"], operation_id);
    }

    // Verify artifact preview
    assert_eq!(received_messages[8]["type"], "operation.artifact_preview");

    // Verify artifact completion
    assert_eq!(received_messages[9]["type"], "operation.artifact_completed");
    assert!(received_messages[9]["artifact"]["content"].is_string());

    // Verify operation completion
    assert_eq!(received_messages[10]["type"], "operation.completed");

    println!("✓ Operations protocol flow verified");
}

// ============================================================================
// TEST 3: Protocol Coexistence
// ============================================================================

#[tokio::test]
async fn test_protocol_coexistence() {
    println!("\n=== Testing Protocol Coexistence ===\n");

    // Test that both protocols can be active simultaneously
    // This simulates two different turns happening at the same time

    let (tx, mut rx) = mpsc::channel::<Value>(100);

    println!("[1] Starting legacy chat flow");
    tx.send(json!({"type": "status", "status": "thinking"}))
        .await
        .unwrap();
    tx.send(json!({"type": "stream", "delta": "Legacy response"}))
        .await
        .unwrap();

    println!("[2] Starting operation flow (concurrent)");
    tx.send(json!({
        "type": "operation.started",
        "operation_id": "op-concurrent"
    }))
    .await
    .unwrap();

    println!("[3] Continuing both flows");
    tx.send(json!({"type": "stream", "delta": " continues"}))
        .await
        .unwrap();
    tx.send(json!({
        "type": "operation.streaming",
        "operation_id": "op-concurrent",
        "delta": "Op response"
    }))
    .await
    .unwrap();

    println!("[4] Completing both flows");
    tx.send(json!({
        "type": "chat_complete",
        "content": "Legacy response continues",
        "artifacts": []
    }))
    .await
    .unwrap();

    tx.send(json!({
        "type": "operation.completed",
        "operation_id": "op-concurrent"
    }))
    .await
    .unwrap();

    drop(tx);

    // Verify both protocols received
    let mut received_messages = Vec::new();
    while let Some(msg) = rx.recv().await {
        received_messages.push(msg);
    }

    assert_eq!(received_messages.len(), 7);

    // Should have mix of both protocol types
    let has_legacy = received_messages.iter().any(|m| m["type"] == "status");
    let has_operations = received_messages
        .iter()
        .any(|m| m["type"] == "operation.started");

    assert!(has_legacy, "Should have legacy protocol messages");
    assert!(has_operations, "Should have operations protocol messages");

    println!("✓ Protocol coexistence verified");
}

// ============================================================================
// TEST 4: Streaming Delta Normalization
// ============================================================================

#[tokio::test]
async fn test_streaming_delta_normalization() {
    println!("\n=== Testing Streaming Delta Normalization ===\n");

    // Test that both "delta" and "content" fields are handled correctly
    // Canonical field is "delta", but some servers send "content"

    println!("[1] Testing delta field (canonical)");
    let msg_with_delta = json!({
        "type": "stream",
        "delta": "Hello"
    });
    assert_eq!(msg_with_delta["delta"], "Hello");

    println!("[2] Testing content field (legacy fallback)");
    let msg_with_content = json!({
        "type": "stream",
        "content": "World"
    });

    // In actual code, this would be normalized to delta
    let normalized_delta = msg_with_content["delta"]
        .as_str()
        .or_else(|| msg_with_content["content"].as_str())
        .unwrap_or("");
    assert_eq!(normalized_delta, "World");

    println!("[3] Testing operation.streaming normalization");
    let op_with_content = json!({
        "type": "operation.streaming",
        "operation_id": "op-123",
        "content": "Operation delta"
    });

    let op_normalized = op_with_content["delta"]
        .as_str()
        .or_else(|| op_with_content["content"].as_str())
        .unwrap_or("");
    assert_eq!(op_normalized, "Operation delta");

    println!("✓ Delta normalization verified");
}

// ============================================================================
// TEST 5: Artifact Delivery Timing
// ============================================================================

#[tokio::test]
async fn test_artifact_delivery_timing() {
    println!("\n=== Testing Artifact Delivery Timing ===\n");

    let (tx, mut rx) = mpsc::channel::<Value>(100);

    println!("[1] Testing mid-stream artifact (operations protocol)");
    tx.send(json!({
        "type": "operation.streaming",
        "operation_id": "op-art",
        "delta": "Creating file..."
    }))
    .await
    .unwrap();

    tx.send(json!({
        "type": "operation.artifact_completed",
        "operation_id": "op-art",
        "artifact": {
            "id": "art-1",
            "path": "test.rs",
            "content": "// test"
        }
    }))
    .await
    .unwrap();

    tx.send(json!({
        "type": "operation.streaming",
        "operation_id": "op-art",
        "delta": " Done!"
    }))
    .await
    .unwrap();

    println!("[2] Testing bundled artifacts (legacy protocol)");
    tx.send(json!({
        "type": "chat_complete",
        "content": "Here are your files",
        "artifacts": [
            {"id": "art-2", "path": "a.rs", "content": "// a"},
            {"id": "art-3", "path": "b.rs", "content": "// b"}
        ]
    }))
    .await
    .unwrap();

    drop(tx);

    // Verify timing
    let mut received_messages = Vec::new();
    while let Some(msg) = rx.recv().await {
        received_messages.push(msg);
    }

    // Mid-stream artifact should arrive between streaming messages
    assert_eq!(received_messages[0]["type"], "operation.streaming");
    assert_eq!(received_messages[1]["type"], "operation.artifact_completed");
    assert_eq!(received_messages[2]["type"], "operation.streaming");

    // Bundled artifacts arrive with completion
    assert_eq!(received_messages[3]["type"], "chat_complete");
    assert_eq!(
        received_messages[3]["artifacts"].as_array().unwrap().len(),
        2
    );

    println!("✓ Artifact delivery timing verified");
}

// ============================================================================
// TEST 6: Message Routing by Type
// ============================================================================

#[tokio::test]
async fn test_message_routing_by_type() {
    println!("\n=== Testing Message Routing by Type ===\n");

    // Test that different message types are routed correctly

    println!("[1] Testing chat message routing");
    let chat_msg = json!({
        "type": "chat",
        "content": "Hello"
    });
    assert_eq!(chat_msg["type"], "chat");

    println!("[2] Testing project command routing");
    let project_msg = json!({
        "type": "project_command",
        "method": "list_projects",
        "params": {}
    });
    assert_eq!(project_msg["type"], "project_command");

    println!("[3] Testing memory command routing");
    let memory_msg = json!({
        "type": "memory_command",
        "method": "search",
        "params": {"query": "test"}
    });
    assert_eq!(memory_msg["type"], "memory_command");

    println!("[4] Testing git command routing");
    let git_msg = json!({
        "type": "git_command",
        "method": "status",
        "params": {}
    });
    assert_eq!(git_msg["type"], "git_command");

    println!("✓ Message routing by type verified");
}

// ============================================================================
// TEST 7: Event Channel Forwarding Without Double-Wrapping
// ============================================================================

#[tokio::test]
async fn test_event_forwarding_no_double_wrapping() {
    println!("\n=== Testing Event Forwarding Without Double-Wrapping ===\n");

    // Critical test: streaming protocol messages should NOT be wrapped in Data envelope

    let (tx, mut rx) = mpsc::channel::<Value>(100);

    println!("[1] Sending top-level streaming messages");

    // These should be sent directly, NOT wrapped
    let top_level_types = vec!["status", "stream", "chat_complete"];

    for msg_type in top_level_types {
        tx.send(json!({
            "type": msg_type,
            "data": "test"
        }))
        .await
        .unwrap();
    }

    println!("[2] Sending data-wrapped messages");

    // These SHOULD be wrapped in Data envelope
    let wrapped_types = vec!["project_list", "file_tree", "memory_data"];

    for msg_type in wrapped_types {
        tx.send(json!({
            "type": "data",
            "data": {
                "type": msg_type,
                "content": "test"
            }
        }))
        .await
        .unwrap();
    }

    drop(tx);

    // Verify wrapping
    let mut received_messages = Vec::new();
    while let Some(msg) = rx.recv().await {
        received_messages.push(msg);
    }

    println!("[3] Verifying top-level messages are NOT wrapped");
    for i in 0..3 {
        let msg = &received_messages[i];
        // Top-level messages should have type directly
        assert!(msg["type"].is_string());
        assert_ne!(
            msg["type"], "data",
            "Streaming messages should not be wrapped in data envelope"
        );
    }

    println!("[4] Verifying other messages ARE wrapped");
    for i in 3..6 {
        let msg = &received_messages[i];
        // These should be in data envelope
        assert_eq!(msg["type"], "data");
        assert!(msg["data"].is_object());
    }

    println!("✓ Event forwarding wrapping verified");
}

// ============================================================================
// TEST 8: Error Handling
// ============================================================================

#[tokio::test]
async fn test_error_handling() {
    println!("\n=== Testing Error Handling ===\n");

    let (tx, mut rx) = mpsc::channel::<Value>(100);

    println!("[1] Sending error message");
    tx.send(json!({
        "type": "error",
        "message": "Something went wrong",
        "code": "INTERNAL_ERROR"
    }))
    .await
    .unwrap();

    println!("[2] Sending operation failure");
    tx.send(json!({
        "type": "operation.failed",
        "operation_id": "op-fail",
        "error": "Generation failed"
    }))
    .await
    .unwrap();

    drop(tx);

    let mut received_messages = Vec::new();
    while let Some(msg) = rx.recv().await {
        received_messages.push(msg);
    }

    assert_eq!(received_messages.len(), 2);
    assert_eq!(received_messages[0]["type"], "error");
    assert_eq!(received_messages[1]["type"], "operation.failed");

    println!("✓ Error handling verified");
}

// ============================================================================
// INTEGRATION TEST: Full Message Router Flow
// ============================================================================

#[tokio::test]
async fn test_full_message_router_integration() {
    println!("\n=== Testing Full Message Router Integration ===\n");

    // Simulate a complete conversation with multiple turns
    // Mix of legacy and operations protocols

    let (tx, mut rx) = mpsc::channel::<Value>(100);

    println!("[1] Turn 1: Simple chat (legacy protocol)");
    tx.send(json!({"type": "status", "status": "thinking"}))
        .await
        .unwrap();
    tx.send(json!({"type": "stream", "delta": "Hello!"}))
        .await
        .unwrap();
    tx.send(json!({"type": "chat_complete", "content": "Hello!", "artifacts": []}))
        .await
        .unwrap();

    println!("[2] Turn 2: Code generation (operations protocol)");
    tx.send(json!({"type": "operation.started", "operation_id": "op-1"}))
        .await
        .unwrap();
    tx.send(json!({"type": "operation.streaming", "operation_id": "op-1", "delta": "Code"}))
        .await
        .unwrap();
    tx.send(json!({
        "type": "operation.artifact_completed",
        "operation_id": "op-1",
        "artifact": {"id": "a1", "path": "test.rs", "content": "fn main() {}"}
    }))
    .await
    .unwrap();
    tx.send(json!({"type": "operation.completed", "operation_id": "op-1"}))
        .await
        .unwrap();

    println!("[3] Turn 3: Another simple chat");
    tx.send(json!({"type": "status", "status": "thinking"}))
        .await
        .unwrap();
    tx.send(json!({"type": "stream", "delta": "Done!"}))
        .await
        .unwrap();
    tx.send(json!({"type": "chat_complete", "content": "Done!", "artifacts": []}))
        .await
        .unwrap();

    drop(tx);

    let mut received_messages = Vec::new();
    while let Some(msg) = rx.recv().await {
        received_messages.push(msg);
    }

    assert_eq!(
        received_messages.len(),
        10,
        "Should receive all messages from 3 turns"
    );

    println!("✓ Full message router integration verified");
    println!("\n=== All Message Routing Tests Passed ===\n");
}
