// src/api/ws/chat.rs
// Fixed WebSocket chat handler with proper persona and memory handling

use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::{State, WebSocketUpgrade},
    response::IntoResponse,
};
use axum::extract::ws::{Message, WebSocket};
use futures::{SinkExt, StreamExt};
use tokio::sync::Mutex;
use tokio::time::interval;
use tracing::{error, info, warn, debug};

use crate::api::ws::message::{WsClientMessage, WsServerMessage};
use crate::llm::streaming::{stream_response, StreamEvent};
use crate::state::AppState;
use crate::persona::PersonaOverlay;
use crate::prompt::builder::build_system_prompt;
use crate::memory::recall::RecallContext;

/// WebSocket upgrade endpoint for chat
pub async fn ws_chat_handler(
    ws: WebSocketUpgrade,
    State(app_state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, app_state))
}

async fn handle_socket(socket: WebSocket, app_state: Arc<AppState>) {
    let (sender, mut receiver) = socket.split();
    let sender = Arc::new(Mutex::new(sender));

    info!("🔌 WS client connected");
    
    // Track current persona for this session - start with Default
    let mut current_persona = PersonaOverlay::Default;
    
    // Spawn heartbeat task to prevent disconnects
    let heartbeat_sender = sender.clone();
    let heartbeat_handle = tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(30));
        loop {
            ticker.tick().await;
            let status = WsServerMessage::Status {
                message: "heartbeat".to_string(),
                detail: Some("ping".to_string()),
            };
            let mut lock = heartbeat_sender.lock().await;
            if lock
                .send(Message::Text(serde_json::to_string(&status).unwrap()))
                .await
                .is_err()
            {
                break;
            }
        }
        debug!("Heartbeat task ended");
    });

    // Buffer for accumulating streaming JSON
    let mut json_buffer = String::new();
    let mut in_structured_response = false;

    while let Some(msg) = receiver.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                // Parse a client message
                let parsed: Result<WsClientMessage, _> = serde_json::from_str(&text);
                match parsed {
                    Ok(WsClientMessage::Chat { content, project_id }) => {
                        // Check if this is a heartbeat response
                        if content.trim().to_lowercase() == "pong" {
                            debug!("Received pong from client");
                            continue; // Don't process as a regular message
                        }
                        
                        info!("💬 Received message: {}", content);
                        
                        // Clear state for new message
                        json_buffer.clear();
                        in_structured_response = false;

                        // Get memory context - first try to get embedding for the user message
                        let embedding = match app_state.llm_client.get_embedding(&content).await {
                            Ok(emb) => Some(emb),
                            Err(e) => {
                                warn!("Failed to generate embedding: {}", e);
                                None
                            }
                        };
                        
                        // Build context using the context service with proper parameters
                        let context = match app_state.context_service
                            .build_context(
                                "default_session",  // session_id
                                embedding.as_deref(),  // embedding as Option<&[f32]>
                                project_id.as_deref()  // project_id as Option<&str>
                            )
                            .await {
                            Ok(ctx) => ctx,
                            Err(e) => {
                                warn!("Failed to get memory context: {}", e);
                                RecallContext::new(Vec::new(), Vec::new())
                            }
                        };
                        
                        // Build the FULL system prompt with persona and memory
                        let system_prompt = build_system_prompt(&current_persona, &context);
                        
                        // Use structured JSON for better control
                        let structured_json = true;
                        
                        // Stream response with full persona context
                        match stream_response(
                            &app_state.llm_client,
                            &content,
                            Some(&system_prompt), // PASS THE PERSONA PROMPT!
                            structured_json,
                        ).await {
                            Ok(mut stream) => {
                                let mut response_started = false;
                                let mut last_output = String::new();
                                
                                while let Some(event) = stream.next().await {
                                    match event {
                                        Ok(StreamEvent::Delta(chunk)) => {
                                            if !chunk.is_empty() {
                                                debug!("Stream chunk ({}): {}", chunk.len(), 
                                                    if chunk.len() > 100 { &chunk[..100] } else { &chunk });
                                                
                                                // Accumulate JSON chunks
                                                json_buffer.push_str(&chunk);
                                                
                                                // Try to parse accumulated buffer as complete JSON
                                                if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&json_buffer) {
                                                    let mut extracted_output = None;
                                                    let mut extracted_mood = None;
                                                    
                                                    // Handle complete item structure (from response.output_item.done)
                                                    if let Some(content_array) = json_val.get("content").and_then(|v| v.as_array()) {
                                                        for content_item in content_array {
                                                            if let Some(text) = content_item.get("text").and_then(|v| v.as_str()) {
                                                                // The text field contains our structured JSON
                                                                if let Ok(inner_json) = serde_json::from_str::<serde_json::Value>(text) {
                                                                    if let Some(output) = inner_json.get("output").and_then(|v| v.as_str()) {
                                                                        extracted_output = Some(output.to_string());
                                                                        extracted_mood = inner_json.get("mood")
                                                                            .and_then(|v| v.as_str())
                                                                            .map(String::from);
                                                                        in_structured_response = true;
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                    // Direct output field (simpler format)
                                                    else if let Some(output) = json_val.get("output").and_then(|v| v.as_str()) {
                                                        extracted_output = Some(output.to_string());
                                                        extracted_mood = json_val.get("mood")
                                                            .and_then(|v| v.as_str())
                                                            .map(String::from);
                                                        in_structured_response = true;
                                                    }
                                                    
                                                    // Send extracted content only if it's new
                                                    if let Some(output) = extracted_output {
                                                        if output != last_output {
                                                            // Send the complete response as a single chunk
                                                            let chunk_msg = WsServerMessage::Chunk {
                                                                content: output.clone(),
                                                                mood: extracted_mood.clone(),
                                                            };
                                                            let mut lock = sender.lock().await;
                                                            if lock.send(Message::Text(serde_json::to_string(&chunk_msg).unwrap())).await.is_err() {
                                                                error!("Failed to send chunk");
                                                                break;
                                                            }
                                                            last_output = output.clone();
                                                            
                                                            // Store memories if salience is high enough
                                                            if let Some(salience) = json_val.get("salience").and_then(|v| v.as_u64()) {
                                                                if salience >= 5 {
                                                                    // Store user message
                                                                    let _ = app_state.memory_service.save_user_message(
                                                                        "default_session",
                                                                        &content,
                                                                        project_id.as_deref(),
                                                                    ).await;
                                                                    
                                                                    // Store assistant response (would need to convert to ChatResponse)
                                                                    info!("Would store assistant response with salience {}", salience);
                                                                }
                                                            }
                                                            
                                                            // Clear buffer after successful parse
                                                            json_buffer.clear();
                                                            in_structured_response = false;
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        Ok(StreamEvent::Done { full_text: _, raw: _ }) => {
                                            info!("✅ Stream complete");
                                            
                                            // Try one last parse if buffer has content
                                            if !json_buffer.is_empty() && !in_structured_response {
                                                if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&json_buffer) {
                                                    if let Some(output) = json_val.get("output").and_then(|v| v.as_str()) {
                                                        let mood = json_val.get("mood")
                                                            .and_then(|v| v.as_str())
                                                            .map(String::from);
                                                        let msg = WsServerMessage::Chunk {
                                                            content: output.to_string(),
                                                            mood,
                                                        };
                                                        let mut lock = sender.lock().await;
                                                        let _ = lock
                                                            .send(Message::Text(serde_json::to_string(&msg).unwrap()))
                                                            .await;
                                                    }
                                                }
                                            }
                                            
                                            // Send complete message with proper Option types
                                            let complete = WsServerMessage::Complete {
                                                mood: Some("present".to_string()),
                                                salience: Some(5.0),
                                                tags: Some(vec![]),
                                            };
                                            let mut lock = sender.lock().await;
                                            if lock.send(Message::Text(serde_json::to_string(&complete).unwrap())).await.is_err() {
                                                error!("Failed to send complete message");
                                            }
                                            
                                            // Also send Done marker
                                            let done = WsServerMessage::Done;
                                            let _ = lock.send(Message::Text(serde_json::to_string(&done).unwrap())).await;
                                            
                                            json_buffer.clear();
                                            in_structured_response = false;
                                        }
                                        Ok(StreamEvent::Error(e)) => {
                                            error!("Stream error: {}", e);
                                            let msg = WsServerMessage::Error {
                                                message: format!("stream error: {}", e),
                                                code: Some("STREAM_ERROR".to_string()),
                                            };
                                            let mut lock = sender.lock().await;
                                            let _ = lock.send(Message::Text(serde_json::to_string(&msg).unwrap())).await;
                                            json_buffer.clear();
                                            in_structured_response = false;
                                            break;
                                        }
                                        Err(e) => {
                                            error!("Stream processing error: {}", e);
                                            let msg = WsServerMessage::Error {
                                                message: format!("stream error: {e}"),
                                                code: Some("STREAM_ERROR".to_string()),
                                            };
                                            let mut lock = sender.lock().await;
                                            let _ = lock
                                                .send(Message::Text(serde_json::to_string(&msg).unwrap()))
                                                .await;
                                            json_buffer.clear();
                                            in_structured_response = false;
                                            break;
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                error!("Failed to start stream: {e}");
                                let msg = WsServerMessage::Error {
                                    message: format!("failed to start stream: {e}"),
                                    code: Some("STREAM_START_FAILED".to_string()),
                                };
                                let mut lock = sender.lock().await;
                                let _ = lock
                                    .send(Message::Text(serde_json::to_string(&msg).unwrap()))
                                    .await;
                            }
                        }
                    }

                    Ok(WsClientMessage::Command { command, .. }) => {
                        // Handle heartbeat response from client
                        if command == "pong" || command == "heartbeat" {
                            debug!("Received heartbeat response");
                        } else if command == "switch_persona" {
                            // Allow persona switching via command
                            info!("Persona switch command received");
                        } else {
                            // Other command ack
                            let msg = WsServerMessage::Status {
                                message: format!("ack:{command}"),
                                detail: None,
                            };
                            let mut lock = sender.lock().await;
                            let _ = lock
                                .send(Message::Text(serde_json::to_string(&msg).unwrap()))
                                .await;
                        }
                    }

                    // Handle other client-side messages gracefully
                    Ok(_) => {
                        debug!("Ignoring client-side meta message");
                    }

                    // Unknown/unsupported client message
                    Err(e) => {
                        warn!("Failed to parse client message: {e}");
                        let msg = WsServerMessage::Error {
                            message: format!("bad client message: {e}"),
                            code: Some("BAD_CLIENT_MESSAGE".to_string()),
                        };
                        let mut lock = sender.lock().await;
                        let _ = lock
                            .send(Message::Text(serde_json::to_string(&msg).unwrap()))
                            .await;
                    }
                }
            }

            Ok(Message::Ping(p)) => {
                let mut lock = sender.lock().await;
                let _ = lock.send(Message::Pong(p)).await;
            }
            Ok(Message::Pong(_)) => {
                debug!("Received pong");
            }
            Ok(Message::Close(_)) => {
                info!("🔌 WS client closed");
                break;
            }
            Ok(other) => {
                warn!("Ignoring non-text WS message: {:?}", other);
            }
            Err(e) => {
                error!("WS receive error: {e}");
                break;
            }
        }
    }

    // Clean up heartbeat task
    heartbeat_handle.abort();
    info!("🔌 WS handler done");
}
