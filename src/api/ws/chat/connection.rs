// src/api/ws/chat/connection.rs
// CLEANED: Professional logging without emojis for terminal-friendly output
// Phase 1: Extract Connection Management from chat.rs
// Handles WebSocket connection state, message sending, and activity tracking

use std::sync::Arc;
use std::time::Instant;

use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use futures_util::stream::SplitSink;
use serde_json::json;
use tokio::sync::Mutex;
use tracing::{debug, error, info};

use crate::api::ws::message::WsServerMessage;
use crate::config::CONFIG;

pub struct WebSocketConnection {
    sender: Arc<Mutex<SplitSink<WebSocket, Message>>>,
    last_activity: Arc<Mutex<Instant>>,
    is_processing: Arc<Mutex<bool>>,
    last_any_send: Arc<Mutex<Instant>>,
}

impl WebSocketConnection {
    pub fn new(socket: WebSocket) -> Self {
        let (sender, _receiver) = socket.split();
        
        Self {
            sender: Arc::new(Mutex::new(sender)),
            last_activity: Arc::new(Mutex::new(Instant::now())),
            is_processing: Arc::new(Mutex::new(false)),
            last_any_send: Arc::new(Mutex::new(Instant::now())),
        }
    }

    pub fn new_with_parts(
        sender: Arc<Mutex<SplitSink<WebSocket, Message>>>,
        last_activity: Arc<Mutex<Instant>>,
        is_processing: Arc<Mutex<bool>>,
        last_any_send: Arc<Mutex<Instant>>,
    ) -> Self {
        Self {
            sender,
            last_activity,
            is_processing,
            last_any_send,
        }
    }

    /// Send a structured WebSocket message
    pub async fn send_message(&self, msg: WsServerMessage) -> Result<(), anyhow::Error> {
        let json_str = serde_json::to_string(&msg)?;
        debug!("Sending WS message: {} bytes", json_str.len());
        
        let mut lock = self.sender.lock().await;
        lock.send(Message::Text(json_str)).await?;
        *self.last_any_send.lock().await = Instant::now();
        
        Ok(())
    }

    /// Send a status message with type "status"
    pub async fn send_status(&self, status: &str) -> Result<(), anyhow::Error> {
        let msg = json!({
            "type": "status",
            "message": status,
            "ts": chrono::Utc::now().to_rfc3339()
        });
        
        debug!("Sending status: {}", status);
        let mut lock = self.sender.lock().await;
        lock.send(Message::Text(msg.to_string())).await?;
        *self.last_any_send.lock().await = Instant::now();
        
        Ok(())
    }

    /// Send an error message with type "error"
    pub async fn send_error(&self, error: &str) -> Result<(), anyhow::Error> {
        let msg = json!({
            "type": "error",
            "message": error,
            "ts": chrono::Utc::now().to_rfc3339()
        });
        
        error!("Sending error: {}", error);
        let mut lock = self.sender.lock().await;
        lock.send(Message::Text(msg.to_string())).await?;
        *self.last_any_send.lock().await = Instant::now();
        
        Ok(())
    }

    /// Send initial connection messages (hello + ready)
    pub async fn send_connection_ready(&self) -> Result<(), anyhow::Error> {
        // Send welcome message
        let welcome_msg = json!({
            "type": "status",
            "message": format!("Connected to Mira v{}", env!("CARGO_PKG_VERSION")),
            "ts": chrono::Utc::now().to_rfc3339()
        });

        // Send config info
        let config_msg = json!({
            "type": "status", 
            "message": format!("Model: {} | Tools: {}", 
                             CONFIG.model, 
                             if CONFIG.enable_chat_tools { "enabled" } else { "disabled" }),
            "ts": chrono::Utc::now().to_rfc3339()
        });

        let mut lock = self.sender.lock().await;
        lock.send(Message::Text(welcome_msg.to_string())).await?;
        lock.send(Message::Text(config_msg.to_string())).await?;
        *self.last_any_send.lock().await = Instant::now();

        info!("WebSocket connection ready messages sent");
        Ok(())
    }

    /// Send pong response to ping
    pub async fn send_pong(&self, data: Vec<u8>) -> Result<(), anyhow::Error> {
        debug!("Received ping, sending pong");
        let mut lock = self.sender.lock().await;
        lock.send(Message::Pong(data)).await?;
        *self.last_any_send.lock().await = Instant::now();
        
        Ok(())
    }

    /// Update last activity timestamp
    pub async fn update_activity(&self) {
        *self.last_activity.lock().await = Instant::now();
    }

    /// Get last activity timestamp
    pub async fn get_last_activity(&self) -> Instant {
        *self.last_activity.lock().await
    }

    /// Check if currently processing a message
    pub async fn is_processing(&self) -> bool {
        *self.is_processing.lock().await
    }

    /// Set processing state
    pub async fn set_processing(&self, processing: bool) {
        *self.is_processing.lock().await = processing;
    }

    /// Get last send timestamp
    pub async fn get_last_send(&self) -> Instant {
        *self.last_any_send.lock().await
    }

    /// Get cloneable references to internal state (for compatibility with existing code)
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::ws::WebSocket;
    
    // Note: WebSocket testing requires more complex setup with actual socket connections
    // These are placeholder tests that would need integration test environment
    
    #[tokio::test]
    async fn test_connection_state_tracking() {
        // This would require a mock WebSocket for proper testing
        // For now, we can test the state management logic
        
        // Test processing state
        let instant = Instant::now();
        let sender = Arc::new(Mutex::new(futures_util::stream::iter(vec![]).split().0));
        let last_activity = Arc::new(Mutex::new(instant));
        let is_processing = Arc::new(Mutex::new(false));
        let last_send = Arc::new(Mutex::new(instant));
        
        let conn = WebSocketConnection::new_with_parts(
            sender,
            last_activity,
            is_processing,
            last_send,
        );
        
        assert!(!conn.is_processing().await);
        conn.set_processing(true).await;
        assert!(conn.is_processing().await);
        conn.set_processing(false).await;
        assert!(!conn.is_processing().await);
    }
}
