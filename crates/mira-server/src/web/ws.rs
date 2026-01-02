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
use tokio::sync::mpsc;

use crate::web::state::AppState;

/// Internal command from recv_task to send_task
enum InternalCmd {
    SyncFrom(Option<i64>),
}

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

    // Channel for internal commands (sync requests)
    let (cmd_tx, mut cmd_rx) = mpsc::channel::<InternalCmd>(16);

    // Get or generate session ID (prefer MCP session if available)
    let session_id = state.session_id.read().await
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    // Send connected event
    let connected = WsEvent::Connected {
        session_id: session_id.clone(),
    };
    if let Ok(msg) = serde_json::to_string(&connected) {
        let _ = sender.send(Message::Text(msg.into())).await;
    }

    // Skip tool history replay for now - causes UI flooding
    // TODO: Batch these or send as single summary event
    // if let Ok(mut history) = state.db.get_session_history(&session_id, 50) {
    //     history.reverse();
    //     replay_history(&mut sender, history).await;
    // }

    // Clone what we need for the send task
    let send_session_id = session_id.clone();
    let send_db = state.db.clone();

    // Spawn task to forward broadcast events and handle sync commands
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
                            continue;
                        }
                        Err(_) => break,
                    }
                }
                // Handle internal commands
                Some(cmd) = cmd_rx.recv() => {
                    match cmd {
                        InternalCmd::SyncFrom(last_event_id) => {
                            let history = if let Some(after_id) = last_event_id {
                                send_db.get_history_after(&send_session_id, after_id, 100)
                            } else {
                                send_db.get_session_history(&send_session_id, 50)
                                    .map(|mut h| { h.reverse(); h })
                            };
                            if let Ok(entries) = history {
                                replay_history(&mut sender, entries).await;
                            }
                        }
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
                                // Pong handled by axum/tungstenite
                            }
                            WsCommand::Sync { last_event_id } => {
                                tracing::debug!("Sync requested from event: {:?}", last_event_id);
                                let _ = cmd_tx.send(InternalCmd::SyncFrom(last_event_id)).await;
                            }
                            WsCommand::Cancel => {
                                tracing::debug!("Cancel requested");
                            }
                        }
                    }
                }
                Ok(Message::Ping(_)) | Ok(Message::Pong(_)) => {}
                Ok(Message::Close(_)) => break,
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

/// Helper to replay history entries to a WebSocket sender
async fn replay_history(
    sender: &mut futures::stream::SplitSink<WebSocket, Message>,
    history: Vec<crate::db::ToolHistoryEntry>,
) {
    // History from get_session_history is DESC, from get_history_after is ASC
    // Caller should ensure correct order
    for entry in history {
        // Reconstruct ToolStart event
        let args: serde_json::Value = entry.arguments
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or(serde_json::Value::Object(Default::default()));

        let start_event = WsEvent::ToolStart {
            tool_name: entry.tool_name.clone(),
            arguments: args,
            call_id: format!("replay-{}", entry.id),
        };
        if let Ok(msg) = serde_json::to_string(&start_event) {
            let _ = sender.send(Message::Text(msg.into())).await;
        }

        // Reconstruct ToolResult event
        let result_event = WsEvent::ToolResult {
            tool_name: entry.tool_name,
            result: entry.result_summary.unwrap_or_default(),
            success: entry.success,
            call_id: format!("replay-{}", entry.id),
            duration_ms: 0,
        };
        if let Ok(msg) = serde_json::to_string(&result_event) {
            let _ = sender.send(Message::Text(msg.into())).await;
        }
    }
}
