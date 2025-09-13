// src/api/ws/chat/mod.rs
// Handles the primary WebSocket chat endpoint, connection lifecycle, and message routing.
// Uses unified handler for all chat processing.

use std::sync::Arc;
use std::time::Instant;

use axum::{
    extract::{ConnectInfo, State, WebSocketUpgrade},
    response::IntoResponse,
};
use axum::extract::ws::{Message, WebSocket};
use futures::StreamExt;
use futures_util::SinkExt;
use futures_util::stream::SplitSink;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

// Module organization for WebSocket chat functionalities
pub mod connection;
pub mod message_router;
pub mod heartbeat;
pub mod unified_handler;

// Re-export key components for easier access from other modules
pub use connection::WebSocketConnection;
pub use message_router::MessageRouter;
pub use heartbeat::HeartbeatManager;
pub use unified_handler::{UnifiedChatHandler, ChatRequest, ChatEvent};

use crate::api::ws::message::{WsClientMessage, WsServerMessage};
use crate::state::AppState;

/// Main WebSocket handler entry point.
/// Upgrades the HTTP connection to a WebSocket and establishes the session.
pub async fn ws_chat_handler(
    ws: WebSocketUpgrade,
    State(app_state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
) -> impl IntoResponse {
    info!("WebSocket upgrade request from {}", addr);
    ws.on_upgrade(move |socket| handle_socket(socket, app_state, addr))
}

/// Manages the entire lifecycle of a single WebSocket connection.
async fn handle_socket(
    socket: WebSocket,
    app_state: Arc<AppState>,
    addr: std::net::SocketAddr,
) {
    let connection_start = Instant::now();
    let (sender, mut receiver) = socket.split();
    
    info!("WebSocket client connected from {}", addr);

    // Atomically shared state for managing the connection's activity
    let last_activity = Arc::new(Mutex::new(Instant::now()));
    let last_any_send = Arc::new(Mutex::new(Instant::now()));
    let is_processing = Arc::new(Mutex::new(false));
    let sender = Arc::new(Mutex::new(sender));

    let connection = Arc::new(WebSocketConnection::new_with_parts(
        sender.clone(),
        last_activity.clone(),
        is_processing.clone(),
        last_any_send.clone(),
    ));

    // Notify the client that the connection is established and ready
    if let Err(e) = connection.send_connection_ready().await {
        error!("Failed to send connection ready message: {}", e);
        return;
    }

    // Initialize and start the heartbeat manager to keep the connection alive
    let heartbeat_manager = Arc::new(HeartbeatManager::new(connection.clone()));
    let heartbeat_handle = tokio::spawn({
        let manager = heartbeat_manager.clone();
        async move {
            if let Err(e) = manager.start().await {
                warn!("Heartbeat manager for client {} exited with error: {}", addr, e);
            }
        }
    });

    // Initialize the message router with unified handler support
    let router = MessageRouter::new(app_state.clone(), connection.clone(), addr);

    // Main message processing loop
    while let Some(result) = receiver.next().await {
        match result {
            Ok(Message::Text(text)) => {
                connection.update_activity().await;
                
                // Parse message with optional request_id
                let (msg, request_id) = match serde_json::from_str::<serde_json::Value>(&text) {
                    Ok(mut json_msg) => {
                        let request_id = json_msg.get("request_id")
                            .and_then(|id| id.as_str())
                            .map(String::from);
                        
                        // Remove request_id before parsing as WsClientMessage
                        if json_msg.get("request_id").is_some() {
                            json_msg.as_object_mut().unwrap().remove("request_id");
                        }
                        
                        match serde_json::from_value::<WsClientMessage>(json_msg) {
                            Ok(msg) => (msg, request_id),
                            Err(e) => {
                                error!("Failed to parse WebSocket message: {}", e);
                                let _ = connection.send_error(
                                    "Invalid message format",
                                    "INVALID_FORMAT".to_string()
                                ).await;
                                continue;
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to parse JSON: {} - Text: {}", e, text);
                        let _ = connection.send_error("Invalid JSON format", "INVALID_JSON".to_string()).await;
                        continue;
                    }
                };
                
                // Route the message through our unified system
                if let Err(e) = router.route_message(msg, request_id).await {
                    error!("Error routing message: {}", e);
                }
            }
            Ok(Message::Binary(data)) => {
                warn!("Received unexpected binary data ({} bytes) from {}", data.len(), addr);
            }
            Ok(Message::Ping(data)) => {
                if let Err(e) = connection.send_pong(data).await {
                    error!("Failed to send pong: {}", e);
                }
            }
            Ok(Message::Pong(_)) => {
                // Pong received, connection is alive
                connection.update_activity().await;
            }
            Ok(Message::Close(_)) => {
                info!("Client {} initiated disconnection", addr);
                break;
            }
            Err(e) => {
                error!("WebSocket transport error for client {}: {}", addr, e);
                break;
            }
        }
    }

    // Cleanup resources on disconnection
    heartbeat_handle.abort();
    info!(
        "WebSocket connection closed for {} after {:?}",
        addr,
        connection_start.elapsed()
    );
}
