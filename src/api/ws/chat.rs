// src/api/ws/chat.rs
// Unified WebSocket chat handler using GPT-5 Responses streaming.

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
                    Ok(WsClientMessage::Chat { content, project_id: _ }) => {
                        // Check if this is a heartbeat response
                        if content.trim().to_lowercase() == "pong" {
                            debug!("Received pong from client");
                            continue; // Don't process as a regular message
                        }
                        
                        info!("💬 Received message: {}", content);
                        
                        // Reset JSON buffer for new message
                        json_buffer.clear();
                        in_structured_response = false;
                        
                        // Start a streaming round-trip
                        let client = &*app_state.llm_client;
                        let structured_json = true; // Keep for metadata extraction

                        match stream_response(client, &content, None, structured_json).await {
                            Ok(mut stream) => {
                                // Notify client that generation started
                                {
                                    let started = WsServerMessage::Status {
                                        message: "started".to_string(),
                                        detail: None,
                                    };
                                    let mut lock = sender.lock().await;
                                    let _ = lock
                                        .send(Message::Text(serde_json::to_string(&started).unwrap()))
                                        .await;
                                }

                                while let Some(next) = stream.next().await {
                                    match next {
                                        Ok(StreamEvent::Delta(chunk)) => {
                                            if chunk.is_empty() {
                                                continue;
                                            }
                                            
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
                                                
                                                // Send the extracted content if we found it
                                                if let Some(output) = extracted_output {
                                                    let msg = WsServerMessage::Chunk {
                                                        content: output,
                                                        mood: extracted_mood,
                                                    };
                                                    let mut lock = sender.lock().await;
                                                    if let Err(e) = lock
                                                        .send(Message::Text(serde_json::to_string(&msg).unwrap()))
                                                        .await
                                                    {
                                                        warn!("WS send error (chunk): {e}");
                                                        break;
                                                    }
                                                    
                                                    // Clear buffer after successful parse and send
                                                    json_buffer.clear();
                                                }
                                            }
                                            // If we can't parse yet, keep accumulating
                                            // But check if buffer is getting too large (safety limit)
                                            else if json_buffer.len() > 50000 {
                                                warn!("JSON buffer too large, clearing");
                                                json_buffer.clear();
                                                in_structured_response = false;
                                            }
                                        }
                                        Ok(StreamEvent::Done { .. }) => {
                                            // Try one more parse if we have buffered content
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
                                            
                                            // Signal completion
                                            let complete = WsServerMessage::Complete {
                                                mood: None,
                                                salience: None,
                                                tags: None,
                                            };
                                            let done = WsServerMessage::Done;
                                            let mut lock = sender.lock().await;
                                            let _ = lock
                                                .send(Message::Text(serde_json::to_string(&complete).unwrap()))
                                                .await;
                                            let _ = lock
                                                .send(Message::Text(serde_json::to_string(&done).unwrap()))
                                                .await;
                                            
                                            // Clear buffer for next message
                                            json_buffer.clear();
                                            in_structured_response = false;
                                            break;
                                        }
                                        Ok(StreamEvent::Error(err_text)) => {
                                            error!("Stream error: {}", err_text);
                                            let msg = WsServerMessage::Error {
                                                message: err_text,
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
