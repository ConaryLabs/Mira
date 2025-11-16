// src/api/ws/chat/mod.rs

use std::sync::Arc;
use std::time::Instant;

use axum::extract::ws::{Message, WebSocket};
use axum::{
    extract::{ConnectInfo, State, WebSocketUpgrade},
    response::IntoResponse,
};
use futures::StreamExt;
use futures_util::SinkExt;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

pub mod connection;
pub mod heartbeat;
pub mod message_router;
pub mod routing;
pub mod unified_handler;

pub use connection::WebSocketConnection;
pub use message_router::MessageRouter;
pub use routing::MessageRouter as LlmMessageRouter;
pub use unified_handler::{ChatRequest, UnifiedChatHandler};

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

async fn handle_socket(socket: WebSocket, app_state: Arc<AppState>, addr: std::net::SocketAddr) {
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

    // Create message router
    let router = MessageRouter::new(app_state.clone(), connection.clone(), addr);

    // Receive loop
    while let Some(result) = receiver.next().await {
        match result {
            Ok(msg) => {
                *last_activity.lock().await = Instant::now();

                match msg {
                    Message::Text(text) => match serde_json::from_str::<WsClientMessage>(&text) {
                        Ok(client_msg) => {
                            *is_processing.lock().await = true;

                            if let Err(e) = router.route_message(client_msg).await {
                                error!("Error routing message: {}", e);
                            }

                            *is_processing.lock().await = false;
                        }
                        Err(e) => {
                            warn!("Failed to parse message: {}", e);
                        }
                    },
                    Message::Ping(data) => {
                        if let Err(e) = sender.lock().await.send(Message::Pong(data)).await {
                            error!("Failed to send pong: {}", e);
                            break;
                        }
                    }
                    Message::Close(_) => {
                        info!("Client initiated close");
                        break;
                    }
                    _ => {}
                }
            }
            Err(e) => {
                error!("WebSocket error: {}", e);
                break;
            }
        }
    }

    // Mark connection as closed (this also stops the heartbeat)
    connection.mark_closed().await;

    let duration = connection_start.elapsed();
    info!(
        "WebSocket client disconnected from {} after {:.2}s",
        addr,
        duration.as_secs_f64()
    );
}
