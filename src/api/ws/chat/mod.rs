// src/api/ws/chat/mod.rs
// Handles the primary WebSocket chat endpoint, connection lifecycle, and message routing.

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
pub use message_router::{MessageRouter, should_use_tools, extract_file_context};
pub use heartbeat::{HeartbeatManager, HeartbeatConfig, HeartbeatStats};

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
            // FIX: Handle the Result from the start method.
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
                match serde_json::from_str::<WsClientMessage>(&text) {
                    Ok(client_msg) => {
                        info!("Received WebSocket message: {:?}", &client_msg);
                        if let Err(e) = message_router.route_message(client_msg).await {
                            error!("Error routing message: {}", e);
                        }
                    }
                    Err(e) => {
                        warn!("Failed to parse client message: {} - Text: {}", e, text);
                        let _ = connection.send_error("Invalid message format").await;
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

    // FIX: Prefix unused variable with underscore
    let _session_id = "websocket_session".to_string();

    let stream = start_response_stream(
        &app_state.llm_client,
        &content,
        Some("You are Mira, a helpful AI assistant. Respond conversationally and naturally."),
        false,
    ).await?;

    handle_stream_response(stream, sender, last_send_ref).await?;

    Ok(())
}

/// Processes a stream of events from the LLM and forwards them to the client.
async fn handle_stream_response(
    mut stream: std::pin::Pin<Box<dyn futures::Stream<Item = Result<StreamEvent, anyhow::Error>> + Send>>,
    sender: Arc<Mutex<SplitSink<WebSocket, Message>>>,
    last_send_ref: Arc<Mutex<Instant>>,
) -> Result<(), anyhow::Error> {
    while let Some(event_result) = stream.next().await {
        match event_result? {
            StreamEvent::Text(text) | StreamEvent::Delta(text) => {
                let msg = WsServerMessage::StreamChunk { text };
                send_ws_message(&msg, &sender).await?;
                update_last_send(last_send_ref.clone()).await;
            }
            StreamEvent::Done { .. } => {
                let end_msg = WsServerMessage::StreamEnd;
                send_ws_message(&end_msg, &sender).await?;
                
                let complete_msg = WsServerMessage::Complete {
                    mood: Some("helpful".to_string()),
                    salience: None,
                    tags: None,
                };
                send_ws_message(&complete_msg, &sender).await?;
                break;
            }
            StreamEvent::Error(error_msg) => {
                error!("Stream error from LLM: {}", error_msg);
                let msg = WsServerMessage::Error { 
                    message: format!("Stream error: {}", error_msg),
                    code: "STREAM_ERROR".to_string(),
                };
                send_ws_message(&msg, &sender).await?;
                break;
            }
        }
    }
    Ok(())
}

/// A thread-safe helper to send a serialized message over the WebSocket.
async fn send_ws_message(
    msg: &WsServerMessage,
    sender: &Arc<Mutex<SplitSink<WebSocket, Message>>>,
) -> Result<(), anyhow::Error> {
    let json = serde_json::to_string(msg)?;
    let ws_msg = Message::Text(json);
    
    if let Ok(mut sender_lock) = sender.try_lock() {
        if let Err(e) = sender_lock.send(ws_msg).await {
            error!("Failed to send WebSocket message: {}", e);
            return Err(e.into());
        }
    } else {
        warn!("Could not acquire WebSocket sender lock to send message.");
    }
    
    Ok(())
}

/// A thread-safe helper to update the timestamp of the last message sent.
async fn update_last_send(last_send_ref: Arc<Mutex<Instant>>) {
    if let Ok(mut last_send) = last_send_ref.try_lock() {
        *last_send = Instant::now();
    }
}
