// src/web/ws.rs
// WebSocket handler for Ghost Mode streaming

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt};
use mira_types::{WsCommand, WsEvent};

use crate::web::state::AppState;

/// WebSocket upgrade handler
pub async fn handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

/// Handle an established WebSocket connection
async fn handle_socket(socket: WebSocket, state: AppState) {
    let (mut sender, mut receiver) = socket.split();

    // Subscribe to broadcast events
    let mut rx = state.ws_tx.subscribe();

    // Generate session ID
    let session_id = uuid::Uuid::new_v4().to_string();

    // Send connected event
    let connected = WsEvent::Connected {
        session_id: session_id.clone(),
    };
    if let Ok(msg) = serde_json::to_string(&connected) {
        let _ = sender.send(Message::Text(msg.into())).await;
    }

    // Spawn task to forward broadcast events to this client
    let send_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                // Forward broadcast events
                result = rx.recv() => {
                    match result {
                        Ok(event) => {
                            if let Ok(msg) = serde_json::to_string(&event) {
                                if sender.send(Message::Text(msg.into())).await.is_err() {
                                    break;
                                }
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                            // Subscriber lagged, continue
                            continue;
                        }
                        Err(_) => break,
                    }
                }
            }
        }
    });

    // Handle incoming messages from client
    let recv_task = tokio::spawn(async move {
        while let Some(result) = receiver.next().await {
            match result {
                Ok(Message::Text(text)) => {
                    if let Ok(cmd) = serde_json::from_str::<WsCommand>(&text) {
                        match cmd {
                            WsCommand::Ping => {
                                // Pong is handled by sending Pong event
                                // (actual pong would require sender access)
                            }
                            WsCommand::Sync { last_event_id } => {
                                // TODO: Implement event replay from database
                                tracing::debug!("Sync requested from event: {:?}", last_event_id);
                            }
                            WsCommand::Cancel => {
                                // TODO: Implement operation cancellation
                                tracing::debug!("Cancel requested");
                            }
                        }
                    }
                }
                Ok(Message::Ping(_)) => {
                    // WebSocket protocol ping - handled automatically by axum
                }
                Ok(Message::Pong(_)) => {
                    // Pong received
                }
                Ok(Message::Close(_)) => {
                    break;
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!("WebSocket error: {}", e);
                    break;
                }
            }
        }
    });

    // Wait for either task to finish
    tokio::select! {
        _ = send_task => {}
        _ = recv_task => {}
    }

    tracing::debug!("WebSocket connection closed: {}", session_id);
}
