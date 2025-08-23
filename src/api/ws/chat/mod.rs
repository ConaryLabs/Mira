// src/api/ws/chat/mod.rs
// REFACTORED VERSION - Phase 4: Simplified Main Handler
// Reduced from ~750 lines to ~200 lines by extracting modules
// 
// EXTRACTED MODULES (now in same directory):
// - connection.rs: WebSocket connection management
// - message_router.rs: Message routing and handling logic  
// - heartbeat.rs: Heartbeat/timeout management
// 
// PRESERVED CRITICAL INTEGRATIONS:
// - chat_tools.rs integration via handle_chat_message_with_tools
// - CONFIG-based routing logic
// - All original message types and parsing
// - Parallel context building and streaming logic

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    extract::{ConnectInfo, State, WebSocketUpgrade},
    response::IntoResponse,
};
use axum::extract::ws::{Message, WebSocket};
use futures::StreamExt;
use futures_util::SinkExt;
use futures_util::stream::SplitSink;
use serde::Deserialize;
use tokio::sync::Mutex;
use tokio::time::timeout;
use tracing::{debug, error, info, warn};

// Import our extracted modules (now in same directory)
pub mod connection;
pub mod message_router;
pub mod heartbeat;

// Re-export for external use
pub use connection::WebSocketConnection;
pub use message_router::{MessageRouter, should_use_tools, extract_file_context};
pub use heartbeat::{HeartbeatManager, HeartbeatConfig, HeartbeatStats};

// Import existing dependencies
use crate::api::ws::message::{WsClientMessage, WsServerMessage};
use crate::llm::streaming::{start_response_stream, StreamEvent};
use crate::state::AppState;
use crate::memory::recall::RecallContext;
use crate::config::CONFIG;

#[derive(Deserialize)]
struct Canary {
    id: String,
    part: u32,
    total: u32,
    complete: bool,
    #[serde(default)]
    done: Option<bool>,
    #[allow(dead_code)]
    msg: Option<String>,
}

/// Main WebSocket handler entry point
pub async fn ws_chat_handler(
    ws: WebSocketUpgrade,
    State(app_state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
) -> impl IntoResponse {
    info!("🔌 WebSocket upgrade request from {}", addr);
    ws.on_upgrade(move |socket| handle_socket(socket, app_state, addr))
}

/// Simplified socket handler using extracted modules
async fn handle_socket(
    socket: WebSocket,
    app_state: Arc<AppState>,
    addr: std::net::SocketAddr,
) {
    let connection_start = Instant::now();
    let (sender, mut receiver) = socket.split();
    
    info!("🔌 WS client connected from {} (new connection)", addr);

    // Create connection wrapper with existing state management
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
        error!("❌ Failed to send connection ready: {}", e);
        return;
    }

    // Create message router
    let router = MessageRouter::new(
        app_state.clone(),
        connection.clone(),
        addr,
    );

    // Start heartbeat manager
    let heartbeat = HeartbeatManager::new(connection.clone());
    let heartbeat_task = heartbeat.start();

    // Main message loop - simplified!
    let receive_timeout = Duration::from_secs(CONFIG.ws_receive_timeout);

    loop {
        let recv_future = timeout(receive_timeout, receiver.next());
        
        match recv_future.await {
            Ok(Some(Ok(msg))) => {
                // Update activity timestamp
                connection.update_activity().await;
                
                match msg {
                    Message::Text(text) => {
                        debug!("📥 Received text message: {} bytes", text.len());

                        // Parse and route messages
                        if let Ok(parsed) = serde_json::from_str::<WsClientMessage>(&text) {
                            if let Err(e) = router.route_message(parsed).await {
                                error!("❌ Error routing message: {}", e);
                            }
                        } else if let Ok(canary) = serde_json::from_str::<Canary>(&text) {
                            debug!("🐤 Canary message: id={}, part={}/{}", 
                                   canary.id, canary.part, canary.total);
                            
                            if canary.complete || canary.done.unwrap_or(false) {
                                info!("🐤 Canary complete");
                            }
                        } else {
                            warn!("❓ Unable to parse message: {}", text);
                        }
                    }
                    Message::Binary(_) => {
                        debug!("📥 Binary message received (ignored)");
                    }
                    Message::Ping(data) => {
                        if let Err(e) = connection.send_pong(data).await {
                            error!("❌ Failed to send pong: {}", e);
                        }
                    }
                    Message::Pong(_) => {
                        debug!("🏓 Pong received");
                    }
                    Message::Close(_) => {
                        info!("🔌 Close frame received");
                        break;
                    }
                }
            }
            Ok(Some(Err(e))) => {
                error!("❌ WebSocket error: {}", e);
                break;
            }
            Ok(None) => {
                info!("🔌 WebSocket stream ended");
                break;
            }
            Err(_) => {
                // Timeout - check if we should break
                if !connection.is_processing().await {
                    warn!("⏱️ WebSocket receive timeout after {:?}", receive_timeout);
                    break;
                }
            }
        }
    }

    // Cleanup
    heartbeat_task.abort();
    let connection_duration = connection_start.elapsed();
    info!("🔌 WebSocket client {} disconnected after {:?}", addr, connection_duration);
}

/// Simple chat handler function (for compatibility with message_router)
pub async fn handle_simple_chat_message(
    content: String,
    _project_id: Option<String>, // Added underscore to suppress warning
    app_state: Arc<AppState>,
    sender: Arc<Mutex<SplitSink<WebSocket, Message>>>,
    _addr: std::net::SocketAddr,
    _last_send: Arc<Mutex<Instant>>,
) -> Result<(), anyhow::Error> {
    info!("💬 Simple chat message: {} chars", content.len());

    // Build basic context
    let _session_id = CONFIG.session_id.clone(); // Added underscore to suppress warning
    let context = RecallContext { recent: vec![], semantic: vec![] };

    // Build system prompt
    let mut sys = String::from("You are Mira. Be concise and stream text output.");
    if !context.recent.is_empty() {
        sys.push_str("\nReference recent context when useful.");
    }

    // Stream response - FIXED: Handle the correct StreamEvent variants and Result wrapper
    let mut stream = start_response_stream(
        &app_state.llm_client,
        &content,
        Some(&sys),
        false,
    ).await?;

    while let Some(event) = stream.next().await {
        match event {
            // FIXED: Use Delta instead of Content and handle Result<StreamEvent>
            Ok(StreamEvent::Delta(text)) => {
                let msg = WsServerMessage::Chunk {
                    content: text,
                    mood: Some("helpful".to_string()),
                };
                
                let json = serde_json::to_string(&msg)?;
                let mut lock = sender.lock().await;
                lock.send(Message::Text(json)).await?;
            }
            // FIXED: Handle Done variant properly (it has fields)
            Ok(StreamEvent::Done { full_text: _, raw: _ }) => {
                let msg = WsServerMessage::Done;
                let json = serde_json::to_string(&msg)?;
                let mut lock = sender.lock().await;
                lock.send(Message::Text(json)).await?;
                break;
            }
            // FIXED: Handle Error variant properly
            Ok(StreamEvent::Error(e)) => {
                error!("❌ Stream error: {}", e);
                let msg = WsServerMessage::Error {
                    message: format!("Stream error: {}", e),
                    code: None,
                };
                let json = serde_json::to_string(&msg)?;
                let mut lock = sender.lock().await;
                lock.send(Message::Text(json)).await?;
                break;
            }
            // FIXED: Handle parsing errors in the Result wrapper
            Err(e) => {
                error!("❌ Stream parsing error: {}", e);
                let msg = WsServerMessage::Error {
                    message: format!("Stream parsing error: {}", e),
                    code: None,
                };
                let json = serde_json::to_string(&msg)?;
                let mut lock = sender.lock().await;
                lock.send(Message::Text(json)).await?;
                break;
            }
        }
    }

    Ok(())
}
