// src/api/ws/chat.rs
use axum::{
    extract::{WebSocketUpgrade, State},
    response::IntoResponse,
};
use axum::extract::ws::{WebSocket, Message};
use futures::{sink::SinkExt, stream::StreamExt};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{timeout, Duration};

use crate::state::AppState;
use crate::api::ws::message::{WsClientMessage, WsServerMessage};
use crate::persona::PersonaOverlay;

/// Main WebSocket handler for chat connections
pub async fn ws_chat_handler(
    ws: WebSocketUpgrade,
    State(app_state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, app_state))
}

/// Handles an individual WebSocket connection
async fn handle_socket(socket: WebSocket, app_state: Arc<AppState>) {
    let session_id = "peter-eternal".to_string();

    let (sender, mut receiver) = socket.split();
    let sender = Arc::new(Mutex::new(sender));

    // Track connection state
    let mut current_mood = "attentive".to_string();
    let current_persona = PersonaOverlay::Default;
    let mut active_project_id: Option<String> = None;

    // Send initial greeting chunk
    let greeting_msg = WsServerMessage::Chunk {
        content: "Connected! How can I help you today?".to_string(),
        mood: Some("attentive".to_string()),
    };

    {
        let mut sender_guard = sender.lock().await;
        if sender_guard
            .send(Message::Text(serde_json::to_string(&greeting_msg).unwrap()))
            .await
            .is_err()
        {
            return;
        }
    }

    // Setup heartbeat
    let sender_clone = sender.clone();
    let (heartbeat_shutdown_tx, mut heartbeat_shutdown_rx) = tokio::sync::mpsc::channel::<()>(1);
    let heartbeat_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    let mut sender_guard = sender_clone.lock().await;
                    if sender_guard.send(Message::Ping(vec![])).await.is_err() {
                        break;
                    }
                    drop(sender_guard);
                }
                _ = heartbeat_shutdown_rx.recv() => {
                    break;
                }
            }
        }
    });

    // Main message handling loop
    while let Some(msg) = receiver.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                let incoming: Result<WsClientMessage, _> = serde_json::from_str(&text);
                match incoming {
                    Ok(WsClientMessage::Message { content, persona: _, project_id }) => {
                        // Update active project if specified
                        if project_id.is_some() {
                            active_project_id = project_id.clone();
                        }

                        // Send immediate acknowledgment (typing indicator)
                        let thinking_msg = WsServerMessage::Chunk {
                            content: "".to_string(),
                            mood: Some("thinking".to_string()),
                        };

                        {
                            let mut sender_guard = sender.lock().await;
                            if sender_guard
                                .send(Message::Text(serde_json::to_string(&thinking_msg).unwrap()))
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }

                        // Process with GPT-5-based ChatService (handles memory/embeddings/tooling)
                        let response = match timeout(
                            Duration::from_secs(30),
                            app_state.chat_service.process_message_gpt5(
                                session_id.as_str(),
                                &content,
                                &current_persona,
                                active_project_id.as_deref(),
                                None,  // images
                                None,  // pdfs
                            )
                        ).await {
                            Ok(Ok(resp)) => resp,
                            Ok(Err(_)) => {
                                let error_msg = WsServerMessage::Error {
                                    message: "Failed to process message".to_string(),
                                    code: Some("PROCESSING_ERROR".to_string()),
                                };
                                let mut sender_guard = sender.lock().await;
                                let _ = sender_guard
                                    .send(Message::Text(serde_json::to_string(&error_msg).unwrap()))
                                    .await;
                                continue;
                            }
                            Err(_) => {
                                let error_msg = WsServerMessage::Error {
                                    message: "Request timed out".to_string(),
                                    code: Some("TIMEOUT".to_string()),
                                };
                                let mut sender_guard = sender.lock().await;
                                let _ = sender_guard
                                    .send(Message::Text(serde_json::to_string(&error_msg).unwrap()))
                                    .await;
                                continue;
                            }
                        };

                        // Update mood for UI (fix: assign to String, not &str)
                        current_mood = response.mood.clone();

                        // Sanitize output in case upstream wrapped it in JSON
                        let cleaned_output = extract_user_facing_text(&response.output);
                        let words: Vec<&str> = cleaned_output.split_whitespace().collect();
                        let chunk_size = 5; // Words per chunk

                        for (i, chunk) in words.chunks(chunk_size).enumerate() {
                            let is_first = i == 0;
                            let chunk_text = if is_first {
                                chunk.join(" ")
                            } else {
                                format!(" {}", chunk.join(" "))
                            };

                            let chunk_msg = WsServerMessage::Chunk {
                                content: chunk_text,
                                mood: Some(current_mood.clone()),
                            };

                            {
                                let mut sender_guard = sender.lock().await;
                                if sender_guard
                                    .send(Message::Text(serde_json::to_string(&chunk_msg).unwrap()))
                                    .await
                                    .is_err()
                                {
                                    break;
                                }
                            }

                            // Small delay for natural streaming effect
                            tokio::time::sleep(Duration::from_millis(50)).await;
                        }

                        // Send completion signal
                        let done_msg = WsServerMessage::Done;
                        {
                            let mut sender_guard = sender.lock().await;
                            let _ = sender_guard
                                .send(Message::Text(serde_json::to_string(&done_msg).unwrap()))
                                .await;
                        }
                    }

                    Ok(WsClientMessage::Typing { .. }) => {
                        // no-op
                    }

                    Err(_) => {
                        let error_msg = WsServerMessage::Error {
                            message: "Invalid message format".to_string(),
                            code: Some("PARSE_ERROR".to_string()),
                        };
                        let mut sender_guard = sender.lock().await;
                        if sender_guard
                            .send(Message::Text(serde_json::to_string(&error_msg).unwrap()))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }
            Ok(Message::Pong(_)) => {
                // no-op
            }
            Ok(Message::Close(_)) => {
                break;
            }
            Err(_) => {
                break;
            }
            _ => {}
        }
    }

    // Signal heartbeat to stop
    let _ = heartbeat_shutdown_tx.send(()).await;

    // Wait for heartbeat task to finish (with timeout)
    let _ = tokio::time::timeout(Duration::from_secs(1), heartbeat_handle).await;
}

/// Extractor to strip `json { ... }` or ```json blocks and return user-facing text.
fn extract_user_facing_text(raw: &str) -> String {
    use serde_json::Value;

    let mut s = raw.trim().to_string();

    // ```json ... ``` or ``` ... ```
    if s.starts_with("```") {
        if let Some(start) = s.find('\n') {
            if let Some(end) = s.rfind("```") {
                s = s[start + 1..end].trim().to_string();
            }
        }
    }

    // Leading "json "
    if s.to_ascii_lowercase().starts_with("json ") {
        s = s[4..].trim().to_string();
    }

    // Try to parse as {"response": "..."}
    if let Ok(v) = serde_json::from_str::<Value>(&s) {
        if let Some(resp) = v.get("response").and_then(|x| x.as_str()) {
            return resp.to_string();
        }
    }

    s
}
