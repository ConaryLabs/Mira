// src/api/ws/chat/mod.rs

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    extract::{ConnectInfo, State, WebSocketUpgrade},
    response::IntoResponse,
};
use axum::extract::ws::{Message, WebSocket};
use futures::StreamExt;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

pub mod connection;
pub mod message_router;
pub mod heartbeat;
pub mod unified_handler;

pub use connection::WebSocketConnection;
pub use message_router::MessageRouter;
pub use heartbeat::HeartbeatManager;
pub use unified_handler::{UnifiedChatHandler, ChatRequest};

use crate::api::ws::message::WsClientMessage;
use crate::state::AppState;

pub async fn ws_chat_handler(
    ws: WebSocketUpgrade,
    State(app_state): State<Arc<AppState>>, 
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
) -> impl IntoResponse {
    info!("WebSocket upgrade request from {}", addr);
    ws.on_upgrade(move |socket| handle_socket(socket, app_state, addr))
}

async fn handle_socket(
    socket: WebSocket,
    app_state: Arc<AppState>,
    addr: std::net::SocketAddr,
) {
    let connection_start = Instant::now();
    let (sender, mut receiver) = socket.split();
    
    info!("WebSocket client connected from {}", addr);

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

    if let Err(e) = connection.send_connection_ready().await {
        error!("Failed to send connection ready message: {}", e);
        return;
    }

    // Bridge: HeartbeatManager expects a non-async Fn(&str). We wrap our async send
    // in a spawned task that calls connection.send_status with a heartbeat label.
    let status_sender = {
        let c = connection.clone();
        Arc::new(move |payload: &str| {
            let c = c.clone();
            let payload_owned = payload.to_string();
            tokio::spawn(async move {
                // Ignore send errors if the connection closed mid-flight
                let _ = c.send_status("heartbeat", Some(payload_owned)).await;
            });
        })
    };

    let heartbeat_manager = Arc::new(HeartbeatManager::new(status_sender));
    heartbeat_manager.start(Duration::from_secs(4));

    let message_router = MessageRouter::new(app_state.clone(), connection.clone(), addr);

    while let Some(msg) = receiver.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                connection.update_activity().await;
                
                match serde_json::from_str::<WsClientMessage>(&text) {
                    Ok(client_msg) => {
                        if let Err(e) = message_router.route_message(client_msg, None).await {
                            error!("Error routing message: {}", e);
                            let _ = connection.send_error(&e.to_string(), "ROUTING_ERROR".to_string()).await;
                        }
                    }
                    Err(e) => {
                        warn!("Failed to parse WebSocket message: {} - Error: {}", text, e);
                        let _ = connection.send_error("Invalid message format", "PARSE_ERROR".to_string()).await;
                    }
                }
            }
            Ok(Message::Binary(_)) => {
                warn!("Received binary message, ignoring");
            }
            Ok(Message::Ping(payload)) => {
                if let Err(e) = connection.send_pong(payload).await {
                    error!("Failed to send pong: {}", e);
                    break;
                }
            }
            Ok(Message::Pong(_)) => {
                connection.update_activity().await;
            }
            Ok(Message::Close(_)) => {
                info!("WebSocket connection closed by client");
                break;
            }
            Err(e) => {
                error!("WebSocket error: {}", e);
                break;
            }
        }
    }

    // Clean shutdown of heartbeat task
    heartbeat_manager.stop();
    
    let connection_duration = connection_start.elapsed();
    info!("WebSocket client {} disconnected after {:?}", addr, connection_duration);
}
