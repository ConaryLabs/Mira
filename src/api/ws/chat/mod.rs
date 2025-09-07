// src/api/ws/chat/mod.rs
// Handles the primary WebSocket chat endpoint, connection lifecycle, and message routing.
// UPDATED: Added request_id parsing and passing to message router

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

// Module organization for WebSocket chat functionalities.
pub mod connection;
pub mod message_router;
pub mod heartbeat;

// Re-export key components for easier access from other modules.
pub use connection::WebSocketConnection;
pub use message_router::MessageRouter;
pub use heartbeat::HeartbeatManager;

use crate::api::ws::message::{WsClientMessage, WsServerMessage};
use crate::llm::streaming::{start_response_stream, StreamEvent};
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

    // Atomically shared state for managing the connection's activity.
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

    // Notify the client that the connection is established and ready.
    if let Err(e) = connection.send_connection_ready().await {
        error!("Failed to send connection ready message: {}", e);
        return;
    }

    // Initialize and start the heartbeat manager to keep the connection alive.
    let heartbeat_manager = Arc::new(HeartbeatManager::new(connection.clone()));
    let heartbeat_handle = tokio::spawn({
        let manager = heartbeat_manager.clone();
        async move {
            if let Err(e) = manager.start().await {
                warn!("Heartbeat manager for client {} exited with error: {}", addr, e);
            }
        }
    });

    // Initialize the message router to handle incoming client messages.
    let message_router = MessageRouter::new(
        app_state.clone(),
        connection.clone(),
        addr,
    );

    // Main loop to process incoming messages from the client.
    while let Some(msg) = receiver.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                *last_activity.lock().await = Instant::now();
                
                // Parse the message and extract request_id if present
                match serde_json::from_str::<serde_json::Value>(&text) {
                    Ok(json_msg) => {
                        // Extract request_id before parsing the actual message
                        let request_id = json_msg.get("request_id")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                        
                        // Now parse the actual WebSocket message
                        match serde_json::from_value::<WsClientMessage>(json_msg.clone()) {
                            Ok(client_msg) => {
                                info!("Received WebSocket message: {:?}", &client_msg);
                                // Pass the request_id along with the message
                                if let Err(e) = message_router.route_message(client_msg, request_id).await {
                                    error!("Error routing message: {}", e);
                                }
                            }
                            Err(e) => {
                                warn!("Failed to parse client message: {} - JSON: {:?}", e, json_msg);
                                let _ = connection.send_error("Invalid message format", "INVALID_FORMAT".to_string()).await;
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Failed to parse JSON: {} - Text: {}", e, text);
                        let _ = connection.send_error("Invalid JSON format", "INVALID_JSON".to_string()).await;
                    }
                }
            }
            Ok(Message::Close(_)) => {
                info!("Client {} initiated disconnection", addr);
                break;
            }
            Ok(_) => {}
            Err(e) => {
                error!("WebSocket transport error for client {}: {}", addr, e);
                break;
            }
        }
    }

    // Cleanup resources on disconnection.
    heartbeat_handle.abort();
    info!(
        "WebSocket connection closed for {} after {:?}",
        addr,
        connection_start.elapsed()
    );
}

/// Handles a simple (non-tool-enabled) chat message and streams the response.
pub async fn handle_simple_chat_message(
    content: String,
    _project_id: Option<String>,
    app_state: Arc<AppState>,
    sender: Arc<Mutex<SplitSink<WebSocket, Message>>>,
    last_send_ref: Arc<Mutex<Instant>>,
) -> Result<(), anyhow::Error> {
    info!("Processing simple chat message: {}", content.chars().take(80).collect::<String>());

    let _session_id = "websocket_session".to_string();

    let stream = start_response_stream(
        &app_state.llm_client,
        &content,
        Some("You are Mira, a helpful AI assistant. Respond conversationally and naturally."),
        false,  // structured_json = false for regular chat
    )
    .await?;

    tokio::pin!(stream);

    while let Some(event_result) = stream.next().await {
        match event_result {
            Ok(event) => {
                match event {
                    // Handle both Delta and Text variants (they're both text chunks)
                    StreamEvent::Delta(text) | StreamEvent::Text(text) => {
                        let msg = WsServerMessage::StreamChunk { text };
                        let json_str = serde_json::to_string(&msg)?;
                        sender.lock().await.send(Message::Text(json_str)).await?;
                        *last_send_ref.lock().await = Instant::now();
                    }
                    // Handle completion
                    StreamEvent::Done { full_text: _, raw: _ } => {
                        let msg = WsServerMessage::StreamEnd;
                        let json_str = serde_json::to_string(&msg)?;
                        sender.lock().await.send(Message::Text(json_str)).await?;
                        
                        // Send completion metadata
                        let complete_msg = WsServerMessage::Complete {
                            mood: Some("helpful".to_string()),
                            salience: None,
                            tags: None,
                        };
                        let json_str = serde_json::to_string(&complete_msg)?;
                        sender.lock().await.send(Message::Text(json_str)).await?;
                        *last_send_ref.lock().await = Instant::now();
                        break;
                    }
                    // Handle errors from the stream
                    StreamEvent::Error(e) => {
                        error!("Stream error: {}", e);
                        let msg = WsServerMessage::Error {
                            message: format!("Stream error: {e}"),
                            code: "STREAM_ERROR".to_string(),
                        };
                        let json_str = serde_json::to_string(&msg)?;
                        sender.lock().await.send(Message::Text(json_str)).await?;
                        *last_send_ref.lock().await = Instant::now();
                        break;
                    }
                }
            }
            Err(e) => {
                // Handle Result errors from the stream
                error!("Stream result error: {}", e);
                let msg = WsServerMessage::Error {
                    message: format!("Stream processing error: {e}"),
                    code: "STREAM_RESULT_ERROR".to_string(),
                };
                let json_str = serde_json::to_string(&msg)?;
                sender.lock().await.send(Message::Text(json_str)).await?;
                *last_send_ref.lock().await = Instant::now();
                break;
            }
        }
    }

    Ok(())
}
