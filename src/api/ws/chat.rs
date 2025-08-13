// src/api/ws/chat.rs
// Phase 7: Unified WebSocket chat handler using ChatService

use axum::{
    extract::{WebSocketUpgrade, State},
    response::IntoResponse,
};
use axum::extract::ws::{WebSocket, Message};
use futures::{sink::SinkExt, stream::StreamExt};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{timeout, Duration};
use serde_json::json;
use tracing::{info, warn, error};

use crate::state::AppState;
use crate::api::ws::message::{WsClientMessage, WsServerMessage};
use crate::api::ws::session_state::WsSessionState;

/// Main WebSocket handler for chat connections
pub async fn ws_chat_handler(
    ws: WebSocketUpgrade,
    State(app_state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, app_state))
}

/// Handles an individual WebSocket connection
async fn handle_socket(socket: WebSocket, app_state: Arc<AppState>) {
    // Use the same session ID pattern as REST
    let session_id = "peter-eternal".to_string();
    info!("🔌 WebSocket connection established for session: {}", session_id);

    let (sender, mut receiver) = socket.split();
    let sender = Arc::new(Mutex::new(sender));

    // Track connection state
    let mut ws_state = WsSessionState::new(session_id.clone());
    let mut current_mood = "attentive".to_string();

    // Send initial greeting
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
                }
                _ = heartbeat_shutdown_rx.recv() => {
                    break;
                }
            }
        }
    });

    // Main message loop
    while let Some(Ok(msg)) = receiver.next().await {
        match msg {
            Message::Text(text) => {
                // Parse incoming message
                let client_msg: WsClientMessage = match serde_json::from_str(&text) {
                    Ok(msg) => msg,
                    Err(e) => {
                        warn!("Failed to parse WebSocket message: {}", e);
                        let error_msg = WsServerMessage::Error {
                            message: "Invalid message format".to_string(),
                            code: Some("PARSE_ERROR".to_string()),
                        };
                        let mut sender_guard = sender.lock().await;
                        let _ = sender_guard
                            .send(Message::Text(serde_json::to_string(&error_msg).unwrap()))
                            .await;
                        continue;
                    }
                };

                match client_msg {
                    WsClientMessage::Chat { content, project_id } => {
                        info!("💬 Processing chat message via unified ChatService");
                        
                        // Update session state
                        ws_state.set_project(project_id.clone());
                        ws_state.mark_active();

                        // Send typing indicator
                        let typing_msg = WsServerMessage::Status {
                            message: "thinking".to_string(),
                            detail: Some("Processing your message...".to_string()),
                        };
                        {
                            let mut sender_guard = sender.lock().await;
                            let _ = sender_guard
                                .send(Message::Text(serde_json::to_string(&typing_msg).unwrap()))
                                .await;
                        }

                        // Call the unified ChatService with timeout
                        let chat_future = app_state.chat_service.process_message(
                            &session_id,
                            &content,
                            project_id.as_deref(),
                            true, // Request structured JSON for full response data
                        );

                        let response = match timeout(Duration::from_secs(30), chat_future).await {
                            Ok(Ok(chat_response)) => {
                                // Update mood from response
                                current_mood = chat_response.mood.clone();
                                ws_state.set_mood(current_mood.clone());
                                
                                chat_response
                            }
                            Ok(Err(e)) => {
                                error!("ChatService error: {:?}", e);
                                let error_msg = WsServerMessage::Error {
                                    message: "Failed to process message".to_string(),
                                    code: Some("CHAT_ERROR".to_string()),
                                };
                                let mut sender_guard = sender.lock().await;
                                let _ = sender_guard
                                    .send(Message::Text(serde_json::to_string(&error_msg).unwrap()))
                                    .await;
                                continue;
                            }
                            Err(_) => {
                                warn!("Chat request timed out");
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

                        // Stream the response in chunks for natural feel
                        stream_response(&sender, &response.output, &current_mood).await;

                        // Send completion message with metadata
                        let completion_msg = WsServerMessage::Complete {
                            mood: Some(response.mood),
                            salience: Some(response.salience as f32),
                            tags: if response.tags.is_empty() { None } else { Some(response.tags) },
                        };
                        
                        let mut sender_guard = sender.lock().await;
                        let _ = sender_guard
                            .send(Message::Text(serde_json::to_string(&completion_msg).unwrap()))
                            .await;
                    }
                    
                    WsClientMessage::Command { command, args } => {
                        info!("📟 Processing command: {}", command);
                        handle_command(&sender, &command, args, &mut ws_state, &app_state).await;
                    }
                    
                    WsClientMessage::Status { .. } => {
                        // Echo back status
                        ws_state.mark_active();
                        let status_msg = WsServerMessage::Status {
                            message: "acknowledged".to_string(),
                            detail: None,
                        };
                        let mut sender_guard = sender.lock().await;
                        let _ = sender_guard
                            .send(Message::Text(serde_json::to_string(&status_msg).unwrap()))
                            .await;
                    }
                }
            }
            
            Message::Ping(data) => {
                let mut sender_guard = sender.lock().await;
                let _ = sender_guard.send(Message::Pong(data)).await;
                ws_state.mark_active();
            }
            
            Message::Pong(_) => {
                ws_state.mark_active();
            }
            
            Message::Close(_) => {
                info!("🔌 WebSocket connection closing for session: {}", session_id);
                break;
            }
            
            _ => {}
        }
    }

    // Cleanup
    let _ = heartbeat_shutdown_tx.send(()).await;
    heartbeat_handle.abort();
    info!("✅ WebSocket connection closed for session: {}", session_id);
}

/// Stream response text in chunks for natural conversation feel
async fn stream_response(sender: &Arc<Mutex<WebSocket>>, text: &str, mood: &str) {
    let words: Vec<&str> = text.split_whitespace().collect();
    let chunk_size = 5; // words per chunk

    for (i, chunk) in words.chunks(chunk_size).enumerate() {
        let is_first = i == 0;
        let chunk_text = if is_first {
            chunk.join(" ")
        } else {
            format!(" {}", chunk.join(" "))
        };

        let chunk_msg = WsServerMessage::Chunk {
            content: chunk_text,
            mood: if is_first { Some(mood.to_string()) } else { None },
        };

        let mut sender_guard = sender.lock().await;
        if sender_guard
            .send(Message::Text(serde_json::to_string(&chunk_msg).unwrap()))
            .await
            .is_err()
        {
            break;
        }

        // Small delay between chunks for natural feel
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Handle WebSocket commands
async fn handle_command(
    sender: &Arc<Mutex<WebSocket>>,
    command: &str,
    args: Option<serde_json::Value>,
    ws_state: &mut WsSessionState,
    app_state: &Arc<AppState>,
) {
    match command {
        "ping" => {
            let pong_msg = WsServerMessage::Status {
                message: "pong".to_string(),
                detail: Some(format!("Session: {}", ws_state.session_id)),
            };
            let mut sender_guard = sender.lock().await;
            let _ = sender_guard
                .send(Message::Text(serde_json::to_string(&pong_msg).unwrap()))
                .await;
        }
        
        "set_project" => {
            if let Some(project_id) = args.and_then(|a| a.get("project_id").and_then(|p| p.as_str())) {
                ws_state.set_project(Some(project_id.to_string()));
                info!("📁 Project set to: {}", project_id);
                
                let status_msg = WsServerMessage::Status {
                    message: "project_set".to_string(),
                    detail: Some(format!("Active project: {}", project_id)),
                };
                let mut sender_guard = sender.lock().await;
                let _ = sender_guard
                    .send(Message::Text(serde_json::to_string(&status_msg).unwrap()))
                    .await;
            }
        }
        
        "get_status" => {
            let status = json!({
                "session_id": ws_state.session_id,
                "current_mood": ws_state.current_mood,
                "active_project": ws_state.active_project_id,
                "last_active": ws_state.last_active.to_rfc3339(),
            });
            
            let status_msg = WsServerMessage::Status {
                message: "session_status".to_string(),
                detail: Some(status.to_string()),
            };
            let mut sender_guard = sender.lock().await;
            let _ = sender_guard
                .send(Message::Text(serde_json::to_string(&status_msg).unwrap()))
                .await;
        }
        
        _ => {
            warn!("Unknown command: {}", command);
            let error_msg = WsServerMessage::Error {
                message: format!("Unknown command: {}", command),
                code: Some("UNKNOWN_COMMAND".to_string()),
            };
            let mut sender_guard = sender.lock().await;
            let _ = sender_guard
                .send(Message::Text(serde_json::to_string(&error_msg).unwrap()))
                .await;
        }
    }
}
