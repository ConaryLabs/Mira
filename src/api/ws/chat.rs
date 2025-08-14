// src/api/ws/chat.rs
// Phase 7: Unified WebSocket chat handler using ChatService + real streaming

use axum::{
    extract::{WebSocketUpgrade, State},
    response::IntoResponse,
};
use axum::extract::ws::{Message, WebSocket};
use futures::{sink::SinkExt, stream::StreamExt};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{Duration};
use tracing::{debug, error, info, warn};

use crate::api::ws::message::{WsClientMessage, WsServerMessage};
use crate::api::ws::session_state::WsSessionState;
use crate::state::AppState;

use crate::llm::streaming::{start_response_stream, StreamEvent};

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
                    WsClientMessage::Command { command, args } => {
                        debug!("🎮 Processing command: {}", command);
                        handle_command(&sender, &command, args, &mut ws_state).await;
                    }

                    WsClientMessage::Chat { content, project_id } => {
                        debug!("💬 Processing chat message");
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

                    // Legacy format support
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

                    WsClientMessage::Typing { .. } => ws_state.mark_active(),
                    WsClientMessage::Status { .. } => ws_state.mark_active(),
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

/// Handle a single user -> assistant turn end-to-end (streaming)
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

    // Start real streaming from GPT-5
    let mut stream = match start_response_stream(
        app_state.llm_client.clone(),          // Arc<OpenAIClient>
        session_id.to_string(),                // session id (String)
        content.clone(),                       // user text (String)
        project_id.clone(),                    // Option<String> project id
        true,                                  // request_structured (JSON downstream)
    ).await {
        Ok(s) => s,
        Err(e) => {
            error!("Chat stream start error: {:?}", e);
            let err = WsServerMessage::Error {
                message: "Failed to process message".to_string(),
                code: Some("CHAT_ERROR".to_string()),
            };
            let mut guard = sender.lock().await;
            let _ = guard
                .send(Message::Text(serde_json::to_string(&err).unwrap()))
                .await;
            return;
        }
    };

    let mut first_chunk = true;
    let mut full_text = String::new();

    while let Some(evt) = stream.next().await {
        match evt {
            Ok(StreamEvent::Delta(token)) => {
                if token.is_empty() {
                    continue;
                }

                let chunk_msg = WsServerMessage::Chunk {
                    content: if first_chunk { token.clone() } else { token.clone() },
                    mood: if first_chunk { Some(current_mood.clone()) } else { None },
                };
                first_chunk = false;

                let mut guard = sender.lock().await;
                if guard
                    .send(Message::Text(serde_json::to_string(&chunk_msg).unwrap()))
                    .await
                    .is_err()
                {
                    break;
                }
                full_text.push_str(&token);
            }

            Ok(StreamEvent::Done { full_text: ft, .. }) => {
                // ensure we have the full text even if last chunk didn’t carry it all
                if full_text.is_empty() {
                    full_text = ft;
                }
                break;
            }

            Ok(StreamEvent::Error(msg)) => {
                error!("Stream error event: {}", msg);
                let err = WsServerMessage::Error {
                    message: "Streaming failed".to_string(),
                    code: Some("STREAM_ERROR".to_string()),
                };
                let mut guard = sender.lock().await;
                let _ = guard
                    .send(Message::Text(serde_json::to_string(&err).unwrap()))
                    .await;
                break;
            }

            Err(e) => {
                warn!("stream error: {e:?}");
                let err = WsServerMessage::Error {
                    message: "Stream error".to_string(),
                    code: Some("STREAM_ERROR".to_string()),
                };
                let mut guard = sender.lock().await;
                let _ = guard
                    .send(Message::Text(serde_json::to_string(&err).unwrap()))
                    .await;
                break;
            }
        }
    }

    // Final completion signal — lightweight (front-end can fetch details if needed)
    let done = WsServerMessage::Complete {
        mood: Some(current_mood.clone()),
        salience: None,
        tags: None,
    };
    let mut guard = sender.lock().await;
    let _ = guard
        .send(Message::Text(serde_json::to_string(&done).unwrap()))
        .await;
}

/// Handle WebSocket commands
async fn handle_command(
    sender: &Arc<Mutex<futures::stream::SplitSink<WebSocket, Message>>>,
    command: &str,
    args: Option<serde_json::Value>,
    ws_state: &mut WsSessionState,
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
            if let Some(project_id) = args.as_ref().and_then(|v| v["project_id"].as_str()) {
                ws_state.set_project(Some(project_id.to_string()));
                let status = WsServerMessage::Status {
                    message: format!("Project set: {}", project_id),
                    detail: None,
                };
                let mut guard = sender.lock().await;
                let _ = guard
                    .send(Message::Text(serde_json::to_string(&status).unwrap()))
                    .await;
            }
        }

        "get_status" => {
            let status = WsServerMessage::Status {
                message: "Connected".to_string(),
                detail: Some(json!({
                    "session_id": ws_state.session_id,
                    "project": ws_state.active_project_id.as_ref(),
                    "mood": ws_state.current_mood,
                    "last_active": ws_state.last_active.to_rfc3339(),
                })
                .to_string()),
            };
            let mut guard = sender.lock().await;
            let _ = guard
                .send(Message::Text(serde_json::to_string(&status).unwrap()))
                .await;
        }

        _ => {
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
