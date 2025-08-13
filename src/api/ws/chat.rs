// src/api/ws/chat.rs
// Phase 7: Unified WebSocket chat handler using ChatService

use axum::{
    extract::{WebSocketUpgrade, State},
    response::IntoResponse,
};
use axum::extract::ws::{Message, WebSocket};
use futures::{sink::SinkExt, stream::StreamExt};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{timeout, Duration};
use tracing::{debug, error, info, warn};

use crate::api::ws::message::{WsClientMessage, WsServerMessage};
use crate::api::ws::session_state::WsSessionState;
use crate::state::AppState;

/// Main WebSocket handler for chat connections
pub async fn ws_chat_handler(
    ws: WebSocketUpgrade,
    State(app_state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, app_state))
}

/// Handles an individual WebSocket connection
async fn handle_socket(socket: WebSocket, app_state: Arc<AppState>) {
    // TODO: plumb a real session id (cookie/query/JWT). For now keep the dev constant.
    let session_id = "peter-eternal".to_string();
    info!("🔌 WebSocket connection established for session: {}", &session_id);

    let (sender, mut receiver) = socket.split();
    let sender = Arc::new(Mutex::new(sender));

    // Track connection state
    let mut ws_state = WsSessionState::new(session_id.clone());
    let mut current_mood = "attentive".to_string();

    // Initial greeting
    {
        let greeting = WsServerMessage::Chunk {
            content: "Connected! How can I help you today?".to_string(),
            mood: Some(current_mood.clone()),
        };
        let mut guard = sender.lock().await;
        if guard
            .send(Message::Text(serde_json::to_string(&greeting).unwrap()))
            .await
            .is_err()
        {
            return;
        }
    }

    // Heartbeat: keep connections healthy behind proxies
    let sender_clone = sender.clone();
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::mpsc::channel::<()>(1);
    let heartbeat_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    let mut guard = sender_clone.lock().await;
                    if guard.send(Message::Ping(vec![])).await.is_err() {
                        break;
                    }
                }
                _ = shutdown_rx.recv() => break,
            }
        }
    });

    // Main loop
    while let Some(Ok(msg)) = receiver.next().await {
        match msg {
            Message::Text(raw) => {
                // Parse client payload
                let client_msg: WsClientMessage = match serde_json::from_str(&raw) {
                    Ok(m) => m,
                    Err(e) => {
                        warn!("Failed to parse WebSocket message: {}", e);
                        let err = WsServerMessage::Error {
                            message: "Invalid message format".to_string(),
                            code: Some("PARSE_ERROR".to_string()),
                        };
                        let mut guard = sender.lock().await;
                        let _ = guard
                            .send(Message::Text(serde_json::to_string(&err).unwrap()))
                            .await;
                        continue;
                    }
                };

                match client_msg {
                    // Preferred modern shape
                    WsClientMessage::Chat { content, project_id } => {
                        info!("💬 Processing chat message via unified ChatService");
                        handle_chat_turn(
                            &app_state,
                            &sender,
                            &mut ws_state,
                            &mut current_mood,
                            &session_id,
                            content,
                            project_id,
                        )
                        .await;
                    }

                    // Commands (debug or control)
                    WsClientMessage::Command { command, args } => {
                        info!("📟 Processing command: {}", command);
                        handle_command(&sender, &command, args, &mut ws_state, &app_state).await;
                    }

                    // Simple acks / keep-alive from client
                    WsClientMessage::Status { .. } => {
                        ws_state.mark_active();
                        let ok = WsServerMessage::Status {
                            message: "acknowledged".to_string(),
                            detail: None,
                        };
                        let mut guard = sender.lock().await;
                        let _ =
                            guard.send(Message::Text(serde_json::to_string(&ok).unwrap())).await;
                    }

                    // Legacy payload (kept for backward compatibility)
                    WsClientMessage::Message { content, project_id, .. } => {
                        debug!("💬 Processing legacy message format");
                        handle_chat_turn(
                            &app_state,
                            &sender,
                            &mut ws_state,
                            &mut current_mood,
                            &session_id,
                            content,
                            project_id,
                        )
                        .await;
                    }

                    WsClientMessage::Typing { .. } => {
                        // No-op; used only to update last-active time
                        ws_state.mark_active();
                    }
                }
            }

            Message::Ping(data) => {
                let mut guard = sender.lock().await;
                let _ = guard.send(Message::Pong(data)).await;
                ws_state.mark_active();
            }

            Message::Pong(_) => ws_state.mark_active(),

            Message::Close(_) => {
                info!("🔌 WebSocket connection closing for session: {}", &session_id);
                break;
            }

            _ => {}
        }
    }

    // Cleanup
    let _ = shutdown_tx.send(()).await;
    heartbeat_handle.abort();
    info!("✅ WebSocket connection closed for session: {}", &session_id);
}

/// Handle a single user -> assistant turn end-to-end
async fn handle_chat_turn(
    app_state: &Arc<AppState>,
    sender: &Arc<Mutex<futures::stream::SplitSink<WebSocket, Message>>>,
    ws_state: &mut WsSessionState,
    current_mood: &mut String,
    session_id: &str,
    content: String,
    project_id: Option<String>,
) {
    // Update session state
    ws_state.set_project(project_id.clone());
    ws_state.mark_active();

    // Typing indicator
    {
        let typing = WsServerMessage::Status {
            message: "thinking".to_string(),
            detail: Some("Processing your message...".to_string()),
        };
        let mut guard = sender.lock().await;
        let _ = guard
            .send(Message::Text(serde_json::to_string(&typing).unwrap()))
            .await;
    }

    // Invoke ChatService (request structured JSON so we can pull mood/tags/salience)
    let fut = app_state.chat_service.process_message(
        session_id,
        &content,
        project_id.as_deref(),
        true,
    );

    let result = match timeout(Duration::from_secs(30), fut).await {
        Ok(Ok(chat_response)) => {
            *current_mood = chat_response.mood.clone();
            ws_state.set_mood(current_mood.clone());
            Some(chat_response)
        }
        Ok(Err(e)) => {
            error!("ChatService error: {:?}", e);
            let err = WsServerMessage::Error {
                message: "Failed to process message".to_string(),
                code: Some("CHAT_ERROR".to_string()),
            };
            let mut guard = sender.lock().await;
            let _ =
                guard.send(Message::Text(serde_json::to_string(&err).unwrap())).await;
            None
        }
        Err(_) => {
            warn!("Chat request timed out");
            let err = WsServerMessage::Error {
                message: "Request timed out".to_string(),
                code: Some("TIMEOUT".to_string()),
            };
            let mut guard = sender.lock().await;
            let _ =
                guard.send(Message::Text(serde_json::to_string(&err).unwrap())).await;
            None
        }
    };

    if let Some(resp) = result {
        // Stream text in small chunks for a natural feel
        stream_response(sender, &resp.output, current_mood).await;

        // Send completion metadata (keep payload small — front-end can request details if needed)
        let done = WsServerMessage::Complete {
            mood: Some(resp.mood),
            salience: Some(resp.salience as f32),
            tags: if resp.tags.is_empty() { None } else { Some(resp.tags) },
        };
        let mut guard = sender.lock().await;
        let _ = guard
            .send(Message::Text(serde_json::to_string(&done).unwrap()))
            .await;
    }
}

/// Stream response text in chunks for natural conversation feel
async fn stream_response(
    sender: &Arc<Mutex<futures::stream::SplitSink<WebSocket, Message>>>,
    text: &str,
    mood: &str,
) {
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

        let mut guard = sender.lock().await;
        if guard
            .send(Message::Text(serde_json::to_string(&chunk_msg).unwrap()))
            .await
            .is_err()
        {
            break;
        }

        // small delay between chunks
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Handle WebSocket commands
async fn handle_command(
    sender: &Arc<Mutex<futures::stream::SplitSink<WebSocket, Message>>>,
    command: &str,
    args: Option<serde_json::Value>,
    ws_state: &mut WsSessionState,
    _app_state: &Arc<AppState>,
) {
    match command {
        "ping" => {
            let pong = WsServerMessage::Status {
                message: "pong".to_string(),
                detail: Some(format!("Session: {}", ws_state.session_id)),
            };
            let mut guard = sender.lock().await;
            let _ = guard
                .send(Message::Text(serde_json::to_string(&pong).unwrap()))
                .await;
        }

        "set_project" => {
            let project_id = args.and_then(|a| a.get("project_id").and_then(|p| p.as_str()).map(|s| s.to_string()));
            if let Some(pid) = project_id {
                ws_state.set_project(Some(pid.clone()));
                info!("📁 Project set to: {}", pid);

                let status = WsServerMessage::Status {
                    message: "project_set".to_string(),
                    detail: Some(format!("Active project: {}", pid)),
                };
                let mut guard = sender.lock().await;
                let _ = guard
                    .send(Message::Text(serde_json::to_string(&status).unwrap()))
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

            let msg = WsServerMessage::Status {
                message: "session_status".to_string(),
                detail: Some(status.to_string()),
            };
            let mut guard = sender.lock().await;
            let _ = guard
                .send(Message::Text(serde_json::to_string(&msg).unwrap()))
                .await;
        }

        _ => {
            warn!("Unknown command: {}", command);
            let err = WsServerMessage::Error {
                message: format!("Unknown command: {}", command),
                code: Some("UNKNOWN_COMMAND".to_string()),
            };
            let mut guard = sender.lock().await;
            let _ = guard
                .send(Message::Text(serde_json::to_string(&err).unwrap()))
                .await;
        }
    }
}
