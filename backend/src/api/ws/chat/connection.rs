// src/api/ws/chat/connection.rs
// A wrapper around the WebSocket connection to manage state and message sending.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use axum::extract::ws::{Message, Utf8Bytes, WebSocket};
use bytes::Bytes;
use futures_util::SinkExt;
use futures_util::stream::{SplitSink, StreamExt};
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

use super::heartbeat::{HeartbeatManager, StatusSender};
use crate::api::ws::message::WsServerMessage;
use crate::config::CONFIG;

/// Manages the state and sending logic for a single WebSocket connection.
pub struct WebSocketConnection {
    sender: Arc<Mutex<SplitSink<WebSocket, Message>>>,
    last_activity: Arc<Mutex<Instant>>,
    is_processing: Arc<Mutex<bool>>,
    last_any_send: Arc<Mutex<Instant>>,
    is_closed: Arc<Mutex<bool>>,
    heartbeat: Arc<HeartbeatManager<WebSocketStatusSender>>,
}

/// Adapter to allow HeartbeatManager to send status messages through WebSocketConnection
struct WebSocketStatusSender {
    sender: Arc<Mutex<SplitSink<WebSocket, Message>>>,
    is_closed: Arc<Mutex<bool>>,
    last_any_send: Arc<Mutex<Instant>>,
}

impl StatusSender for WebSocketStatusSender {
    fn send_status(&self, message: &str) {
        let sender = self.sender.clone();
        let is_closed = self.is_closed.clone();
        let last_any_send = self.last_any_send.clone();
        let message = message.to_string();

        // Spawn async task for sending (heartbeat runs in background)
        tokio::spawn(async move {
            // Check if closed before sending
            if *is_closed.lock().await {
                return;
            }

            let mut sender_guard = sender.lock().await;

            // Send ping frame
            if let Err(e) = sender_guard.send(Message::Text(Utf8Bytes::from(message))).await {
                warn!("Heartbeat send failed (connection likely closed): {}", e);
                return;
            }

            // Flush immediately
            if let Err(e) = sender_guard.flush().await {
                warn!("Heartbeat flush failed: {}", e);
                return;
            }

            drop(sender_guard);
            *last_any_send.lock().await = Instant::now();
            debug!("Heartbeat sent successfully");
        });
    }
}

impl WebSocketConnection {
    /// Creates a new connection from a raw WebSocket socket.
    pub fn new(socket: WebSocket) -> Self {
        let (sender, _receiver) = socket.split();
        let sender = Arc::new(Mutex::new(sender));
        let is_closed = Arc::new(Mutex::new(false));
        let last_any_send = Arc::new(Mutex::new(Instant::now()));

        // Create heartbeat manager with sender adapter
        let status_sender = WebSocketStatusSender {
            sender: sender.clone(),
            is_closed: is_closed.clone(),
            last_any_send: last_any_send.clone(),
        };
        let heartbeat = Arc::new(HeartbeatManager::new(Arc::new(status_sender)));

        Self {
            sender,
            last_activity: Arc::new(Mutex::new(Instant::now())),
            is_processing: Arc::new(Mutex::new(false)),
            last_any_send,
            is_closed,
            heartbeat,
        }
    }

    /// Creates a new connection from its constituent, shared parts.
    pub fn new_with_parts(
        sender: Arc<Mutex<SplitSink<WebSocket, Message>>>,
        last_activity: Arc<Mutex<Instant>>,
        is_processing: Arc<Mutex<bool>>,
        last_any_send: Arc<Mutex<Instant>>,
    ) -> Self {
        let is_closed = Arc::new(Mutex::new(false));

        // Create heartbeat manager
        let status_sender = WebSocketStatusSender {
            sender: sender.clone(),
            is_closed: is_closed.clone(),
            last_any_send: last_any_send.clone(),
        };
        let heartbeat = Arc::new(HeartbeatManager::new(Arc::new(status_sender)));

        Self {
            sender,
            last_activity,
            is_processing,
            last_any_send,
            is_closed,
            heartbeat,
        }
    }

    /// Mark this connection as closed to prevent further sends
    pub async fn mark_closed(&self) {
        *self.is_closed.lock().await = true;

        // Stop heartbeat to prevent sending after close
        self.heartbeat.stop();

        debug!("Connection marked as closed, heartbeat stopped");
    }

    /// Check if connection is closed
    pub async fn is_closed(&self) -> bool {
        *self.is_closed.lock().await
    }

    /// Sends a structured `WsServerMessage` to the client with immediate flushing.
    pub async fn send_message(&self, msg: WsServerMessage) -> Result<()> {
        if self.is_closed().await {
            debug!("Skipping send on closed connection");
            return Ok(());
        }

        let json_str = serde_json::to_string(&msg)?;
        debug!("Sending WS message: {}", json_str);

        let mut sender = self.sender.lock().await;

        if let Err(e) = sender.send(Message::Text(Utf8Bytes::from(json_str))).await {
            warn!("Failed to send message (connection likely closed): {}", e);
            drop(sender);
            self.mark_closed().await;
            return Err(e.into());
        }

        if let Err(e) = sender.flush().await {
            warn!("Failed to flush message (connection likely closed): {}", e);
            drop(sender);
            self.mark_closed().await;
            return Err(e.into());
        }

        drop(sender);

        *self.last_any_send.lock().await = Instant::now();
        Ok(())
    }

    /// Sends a status update message.
    pub async fn send_status(&self, message: &str, detail: Option<String>) -> Result<()> {
        info!("Sending status: {} - {:?}", message, detail);
        self.send_message(WsServerMessage::Status {
            message: message.to_string(),
            detail,
        })
        .await
    }

    /// Sends an error message.
    pub async fn send_error(&self, message: &str, code: String) -> Result<()> {
        error!("Sending error: {} (Code: {})", message, code);
        self.send_message(WsServerMessage::Error {
            message: message.to_string(),
            code,
        })
        .await
    }

    /// Sends initial messages to the client upon connection and starts heartbeat.
    pub async fn send_connection_ready(&self) -> Result<()> {
        let welcome_msg = format!("Connected to Mira v{}", env!("CARGO_PKG_VERSION"));
        let config_msg = format!(
            "Model: OpenAI GPT-5.1 | Tools: {}",
            if CONFIG.enable_chat_tools {
                "enabled"
            } else {
                "disabled"
            }
        );

        // Send all connection messages
        self.send_message(WsServerMessage::Status {
            message: welcome_msg,
            detail: None,
        })
        .await?;

        self.send_message(WsServerMessage::Status {
            message: config_msg,
            detail: None,
        })
        .await?;

        self.send_message(WsServerMessage::ConnectionReady).await?;

        // Start heartbeat with 15-second interval
        // This keeps the connection alive through proxies/load balancers
        self.heartbeat.start(Duration::from_secs(15));

        info!("WebSocket connection ready messages sent, heartbeat started.");
        Ok(())
    }

    /// Sends a pong response to a client's ping with proper flushing.
    pub async fn send_pong(&self, data: Bytes) -> Result<()> {
        if self.is_closed().await {
            debug!("Skipping pong on closed connection");
            return Ok(());
        }

        debug!("Received ping, sending pong.");

        let mut sender = self.sender.lock().await;

        if let Err(e) = sender.send(Message::Pong(data)).await {
            warn!("Failed to send pong: {}", e);
            drop(sender);
            self.mark_closed().await;
            return Err(e.into());
        }

        if let Err(e) = sender.flush().await {
            warn!("Failed to flush pong: {}", e);
            drop(sender);
            self.mark_closed().await;
            return Err(e.into());
        }

        drop(sender);

        *self.last_any_send.lock().await = Instant::now();
        Ok(())
    }

    // Accessors and state management methods
    pub async fn update_activity(&self) {
        *self.last_activity.lock().await = Instant::now();
    }

    pub async fn get_last_activity(&self) -> Instant {
        *self.last_activity.lock().await
    }

    pub async fn is_processing(&self) -> bool {
        *self.is_processing.lock().await
    }

    pub async fn set_processing(&self, processing: bool) {
        *self.is_processing.lock().await = processing;
    }

    pub async fn get_last_send(&self) -> Instant {
        *self.last_any_send.lock().await
    }

    // Getter methods for shared state, useful for creating related components.
    pub fn get_sender(&self) -> Arc<Mutex<SplitSink<WebSocket, Message>>> {
        self.sender.clone()
    }

    pub fn get_last_activity_ref(&self) -> Arc<Mutex<Instant>> {
        self.last_activity.clone()
    }

    pub fn get_is_processing_ref(&self) -> Arc<Mutex<bool>> {
        self.is_processing.clone()
    }

    pub fn get_last_send_ref(&self) -> Arc<Mutex<Instant>> {
        self.last_any_send.clone()
    }

    pub fn get_is_closed_ref(&self) -> Arc<Mutex<bool>> {
        self.is_closed.clone()
    }
}
