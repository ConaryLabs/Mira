// src/api/ws/chat/connection.rs
// A wrapper around the WebSocket connection to manage state and message sending.

use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use axum::extract::ws::{Message, WebSocket};
use futures_util::stream::{SplitSink, StreamExt};
use futures_util::SinkExt;
use tokio::sync::Mutex;
use tracing::{debug, error, info};

use crate::api::ws::message::WsServerMessage;
use crate::config::CONFIG;

/// Manages the state and sending logic for a single WebSocket connection.
pub struct WebSocketConnection {
    sender: Arc<Mutex<SplitSink<WebSocket, Message>>>,
    last_activity: Arc<Mutex<Instant>>,
    is_processing: Arc<Mutex<bool>>,
    last_any_send: Arc<Mutex<Instant>>,
}

impl WebSocketConnection {
    /// Creates a new connection from a raw WebSocket socket.
    pub fn new(socket: WebSocket) -> Self {
        let (sender, _receiver) = socket.split();
        Self {
            sender: Arc::new(Mutex::new(sender)),
            last_activity: Arc::new(Mutex::new(Instant::now())),
            is_processing: Arc::new(Mutex::new(false)),
            last_any_send: Arc::new(Mutex::new(Instant::now())),
        }
    }

    /// Creates a new connection from its constituent, shared parts.
    pub fn new_with_parts(
        sender: Arc<Mutex<SplitSink<WebSocket, Message>>>,
        last_activity: Arc<Mutex<Instant>>,
        is_processing: Arc<Mutex<bool>>,
        last_any_send: Arc<Mutex<Instant>>,
    ) -> Self {
        Self { sender, last_activity, is_processing, last_any_send }
    }

    /// Sends a structured `WsServerMessage` to the client with immediate flushing.
    /// CRITICAL: The flush() ensures messages are sent immediately, preventing
    /// message loss when streaming rapidly.
    pub async fn send_message(&self, msg: WsServerMessage) -> Result<()> {
        let json_str = serde_json::to_string(&msg)?;
        debug!("Sending WS message: {}", json_str);
        
        // Lock the sender and send + flush in one go to ensure atomic operation
        let mut sender = self.sender.lock().await;
        sender.send(Message::Text(json_str)).await?;
        
        // CRITICAL FIX: Force immediate transmission to prevent buffering/dropping
        // Without this, rapid sends (like streaming) will buffer and lose messages
        sender.flush().await?;
        drop(sender); // Explicitly drop to release lock faster
        
        *self.last_any_send.lock().await = Instant::now();
        Ok(())
    }

    /// Sends a status update message.
    pub async fn send_status(&self, message: &str, detail: Option<String>) -> Result<()> {
        info!("Sending status: {} - {:?}", message, detail);
        self.send_message(WsServerMessage::Status { 
            message: message.to_string(), 
            detail 
        }).await
    }

    /// Sends an error message.
    pub async fn send_error(&self, message: &str, code: String) -> Result<()> {
        error!("Sending error: {} (Code: {})", message, code);
        self.send_message(WsServerMessage::Error { 
            message: message.to_string(), 
            code 
        }).await
    }

    /// Sends initial messages to the client upon connection.
    pub async fn send_connection_ready(&self) -> Result<()> {
        let welcome_msg = format!("Connected to Mira v{}", env!("CARGO_PKG_VERSION"));
        let config_msg = format!(
            "Model: {} | Tools: {}",
            CONFIG.anthropic_model,
            if CONFIG.enable_chat_tools { "enabled" } else { "disabled" }
        );

        // Send all connection messages
        self.send_message(WsServerMessage::Status { 
            message: welcome_msg, 
            detail: None 
        }).await?;
        
        self.send_message(WsServerMessage::Status { 
            message: config_msg, 
            detail: None 
        }).await?;
        
        self.send_message(WsServerMessage::ConnectionReady).await?;
        
        info!("WebSocket connection ready messages sent.");
        Ok(())
    }

    /// Sends a pong response to a client's ping with proper flushing.
    pub async fn send_pong(&self, data: Vec<u8>) -> Result<()> {
        debug!("Received ping, sending pong.");
        
        // Pong also needs flushing for reliability
        let mut sender = self.sender.lock().await;
        sender.send(Message::Pong(data)).await?;
        sender.flush().await?;
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
}
