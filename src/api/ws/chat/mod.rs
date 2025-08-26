// src/api/ws/chat/mod.rs - FIXED VERSION

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

// Import extracted modules
pub mod connection;
pub mod message_router;
pub mod heartbeat;

// Re-export for external use
pub use connection::WebSocketConnection;
pub use message_router::{MessageRouter, should_use_tools, extract_file_context};
pub use heartbeat::{HeartbeatManager, HeartbeatConfig, HeartbeatStats};

// Import dependencies
use crate::api::ws::message::{WsClientMessage, WsServerMessage};
use crate::llm::streaming::{start_response_stream, StreamEvent};
use crate::state::AppState;
use crate::memory::recall::RecallContext;

/// Main WebSocket handler entry point - FIXED to accept ConnectInfo
pub async fn ws_chat_handler(
    ws: WebSocketUpgrade,
    State(app_state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
) -> impl IntoResponse {
    info!("WebSocket upgrade request from {}", addr);
    ws.on_upgrade(move |socket| handle_socket(socket, app_state, addr))
}

/// Socket handler using extracted modules - FIXED to accept addr parameter
async fn handle_socket(
    socket: WebSocket,
    app_state: Arc<AppState>,
    addr: std::net::SocketAddr,
) {
    let connection_start = Instant::now();
    let (sender, mut receiver) = socket.split();
    
    info!("WebSocket client connected from {}", addr);

    // Create connection wrapper with state management
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

    // Send initial connection messages
    if let Err(e) = connection.send_connection_ready().await {
        error!("Failed to send connection ready message: {}", e);
        return;
    }

    // Initialize heartbeat manager with just the connection
    let heartbeat_manager = Arc::new(HeartbeatManager::new(
        connection.clone(),
    ));

    // Start heartbeat task
    let heartbeat_handle = tokio::spawn({
        let _heartbeat_manager = heartbeat_manager.clone();
        async move {
            // HeartbeatManager handles its own lifecycle
            tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
        }
    });

    // FIXED: Use real address instead of placeholder
    let message_router = MessageRouter::new(
        app_state.clone(),
        connection.clone(),
        addr, // Use the real client address
    );

    while let Some(msg) = receiver.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                // Update activity timestamp
                {
                    let mut activity_lock = last_activity.lock().await;
                    *activity_lock = Instant::now();
                }

                // Parse and route message
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
                info!("Client {} disconnected", addr);
                break;
            }
            Ok(_) => {
                // Ignore other message types (binary, ping, pong)
            }
            Err(e) => {
                error!("WebSocket error for client {}: {}", addr, e);
                break;
            }
        }
    }

    // Cleanup
    heartbeat_handle.abort();
    info!(
        "WebSocket connection closed for {} after {:?}",
        addr,
        connection_start.elapsed()
    );
}

/// Handle simple chat message (non-tool enabled) - FIXED API
pub async fn handle_simple_chat_message(
    content: String,
    _project_id: Option<String>,
    app_state: Arc<AppState>,
    sender: Arc<Mutex<SplitSink<WebSocket, Message>>>,
    last_send_ref: Arc<Mutex<Instant>>,
) -> Result<(), anyhow::Error> {
    info!("Processing simple chat message: {}", content.chars().take(50).collect::<String>());

    // Build context for the user's message
    let session_id = "websocket_session".to_string();

    // FIXED: Use correct function signature
    let stream = start_response_stream(
        &app_state.llm_client,
        &content,
        Some("You are Mira, a helpful AI assistant. Respond conversationally and naturally."),
        false, // Not structured JSON
    ).await?;

    // Process the stream and send chunks via WebSocket
    handle_stream_response(stream, sender, last_send_ref).await?;

    Ok(())
}

/// Handle streaming response and send to WebSocket - FIXED to match StreamEvent API
async fn handle_stream_response(
    mut stream: std::pin::Pin<Box<dyn futures::Stream<Item = Result<StreamEvent, anyhow::Error>> + Send>>,
    sender: Arc<Mutex<SplitSink<WebSocket, Message>>>,
    last_send_ref: Arc<Mutex<Instant>>,
) -> Result<(), anyhow::Error> {
    while let Some(event_result) = stream.next().await {
        match event_result? {
            // FIXED: Use correct StreamEvent variants
            StreamEvent::Text(text) => {
                let msg = WsServerMessage::StreamChunk { text };
                send_ws_message(&msg, &sender).await?;
                update_last_send(last_send_ref.clone()).await;
            }
            StreamEvent::Delta(delta) => {
                let msg = WsServerMessage::Chunk { 
                    content: delta, 
                    mood: None 
                };
                send_ws_message(&msg, &sender).await?;
                update_last_send(last_send_ref.clone()).await;
            }
            StreamEvent::Done { full_text: _, raw: _ } => {
                let msg = WsServerMessage::StreamEnd;
                send_ws_message(&msg, &sender).await?;
                
                // Send completion message
                let complete_msg = WsServerMessage::Complete {
                    mood: Some("helpful".to_string()),
                    salience: None,
                    tags: None,
                };
                send_ws_message(&complete_msg, &sender).await?;
                break;
            }
            StreamEvent::Error(error_msg) => {
                error!("Stream error: {}", error_msg);
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

/// Helper function to send WebSocket messages
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
        warn!("Failed to acquire sender lock");
    }
    
    Ok(())
}

/// Helper function to update last send timestamp
async fn update_last_send(last_send_ref: Arc<Mutex<Instant>>) {
    if let Ok(mut last_send) = last_send_ref.try_lock() {
        *last_send = Instant::now();
    }
}
