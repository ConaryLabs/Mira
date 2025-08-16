// src/api/ws/chat.rs
// Unified WebSocket chat handler using GPT-5 Responses streaming.

use std::sync::Arc;

use axum::{
    extract::{State, WebSocketUpgrade},
    response::IntoResponse,
};
use axum::extract::ws::{Message, WebSocket};
use futures::{SinkExt, StreamExt};
use tokio::sync::Mutex;
use tracing::{error, info, warn};

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

    while let Some(msg) = receiver.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                // Parse a client message
                let parsed: Result<WsClientMessage, _> = serde_json::from_str(&text);
                match parsed {
                    Ok(WsClientMessage::Chat { content, project_id: _ }) => {
                        // Start a streaming round-trip (inline; 1-at-a-time per connection)
                        let client = &*app_state.llm_client;
                        let structured_json = true;

                        match stream_response(client, &content, None, structured_json).await {
                            Ok(mut stream) => {
                                // notify client that generation started
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
                                            let msg = WsServerMessage::Chunk {
                                                content: chunk,
                                                mood: None,
                                            };
                                            let mut lock = sender.lock().await;
                                            if let Err(e) = lock
                                                .send(Message::Text(serde_json::to_string(&msg).unwrap()))
                                                .await
                                            {
                                                warn!("WS send error (chunk): {e}");
                                                break;
                                            }
                                        }
                                        Ok(StreamEvent::Done { .. }) => {
                                            // Signal completion (no content/raw fields on Complete)
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
                                            break;
                                        }
                                        Ok(StreamEvent::Error(err_text)) => {
                                            let msg = WsServerMessage::Error {
                                                message: err_text,
                                                code: Some("STREAM_ERROR".to_string()),
                                            };
                                            let mut lock = sender.lock().await;
                                            let _ = lock
                                                .send(Message::Text(serde_json::to_string(&msg).unwrap()))
                                                .await;
                                            break;
                                        }
                                        Err(e) => {
                                            let msg = WsServerMessage::Error {
                                                message: format!("stream error: {e}"),
                                                code: Some("STREAM_ERROR".to_string()),
                                            };
                                            let mut lock = sender.lock().await;
                                            let _ = lock
                                                .send(Message::Text(serde_json::to_string(&msg).unwrap()))
                                                .await;
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
                        // Minimal command ack
                        let msg = WsServerMessage::Status {
                            message: format!("ack:{command}"),
                            detail: None,
                        };
                        let mut lock = sender.lock().await;
                        let _ = lock
                            .send(Message::Text(serde_json::to_string(&msg).unwrap()))
                            .await;
                    }

                    // Handle other client-side noisey types gracefully (Status/Message/Typing/etc.)
                    Ok(_) => {
                        let msg = WsServerMessage::Status {
                            message: "ignored".to_string(),
                            detail: Some("client-side meta".to_string()),
                        };
                        let mut lock = sender.lock().await;
                        let _ = lock
                            .send(Message::Text(serde_json::to_string(&msg).unwrap()))
                            .await;
                    }

                    // Unknown/unsupported client message (bad JSON -> parse error)
                    Err(e) => {
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

    info!("🔌 WS handler done");
}
