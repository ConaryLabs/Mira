// src/api/ws/chat/heartbeat.rs
// Phase 3: Extract Heartbeat Management from chat.rs
// Handles dynamic heartbeat intervals, connection timeouts, and ping/pong logic

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use tokio::task::JoinHandle;
use tokio::time::interval;
use tracing::{debug, warn};

// FIXED: Updated import path - now in same directory
use super::connection::WebSocketConnection;
use crate::config::CONFIG;

#[derive(Debug, Clone)]
pub struct HeartbeatConfig {
    /// Base heartbeat interval in seconds
    pub heartbeat_interval: u64,
    /// Connection timeout in seconds
    pub connection_timeout: u64,
    /// Quick heartbeat interval when processing (seconds)
    pub processing_heartbeat_interval: u64,
    /// Recent activity threshold for frequent heartbeats (seconds)
    pub recent_activity_threshold: u64,
    /// Frequent heartbeat interval when recently active (seconds)
    pub frequent_heartbeat_interval: u64,
}

impl HeartbeatConfig {
    /// Create config from global CONFIG
    pub fn from_global_config() -> Self {
        Self {
            heartbeat_interval: CONFIG.ws_heartbeat_interval,
            connection_timeout: CONFIG.ws_connection_timeout,
            processing_heartbeat_interval: 5,
            recent_activity_threshold: 30,
            frequent_heartbeat_interval: 10,
        }
    }

    /// Create config with custom values (for testing)
    pub fn new(
        heartbeat_interval: u64,
        connection_timeout: u64,
    ) -> Self {
        Self {
            heartbeat_interval,
            connection_timeout,
            processing_heartbeat_interval: 5,
            recent_activity_threshold: 30,
            frequent_heartbeat_interval: 10,
        }
    }
}

pub struct HeartbeatManager {
    connection: Arc<WebSocketConnection>,
    config: HeartbeatConfig,
}

impl HeartbeatManager {
    pub fn new(connection: Arc<WebSocketConnection>) -> Self {
        Self {
            connection,
            config: HeartbeatConfig::from_global_config(),
        }
    }

    pub fn new_with_config(
        connection: Arc<WebSocketConnection>,
        config: HeartbeatConfig,
    ) -> Self {
        Self {
            connection,
            config,
        }
    }

    /// Start the heartbeat task in the background
    /// Returns a JoinHandle that can be used to abort the task
    pub fn start(&self) -> JoinHandle<()> {
        let connection = self.connection.clone();
        let config = self.config.clone();

        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(config.heartbeat_interval));
            
            debug!("💓 Heartbeat manager started with interval: {}s", config.heartbeat_interval);
            
            loop {
                ticker.tick().await;
                
                // Check if connection is still processing
                if connection.is_processing().await {
                    debug!("💭 Connection is processing, using quick heartbeat");
                    tokio::time::sleep(Duration::from_secs(config.processing_heartbeat_interval)).await;
                    continue;
                }
                
                // Check last activity for dynamic heartbeat timing
                let last_activity = connection.get_last_activity().await;
                let time_since_activity = last_activity.elapsed();
                
                if time_since_activity.as_secs() < config.recent_activity_threshold {
                    debug!("🔄 Recent activity detected, using frequent heartbeat");
                    tokio::time::sleep(Duration::from_secs(config.frequent_heartbeat_interval)).await;
                    continue;
                }
                
                // Send heartbeat
                if let Err(e) = send_heartbeat(&connection).await {
                    warn!("❌ Failed to send heartbeat: {}", e);
                    break;
                }
                
                // Check for timeout
                let last_send = connection.get_last_send().await;
                let time_since_send = last_send.elapsed();
                
                if time_since_send.as_secs() > config.connection_timeout {
                    warn!("⏰ Connection timeout after {}s", time_since_send.as_secs());
                    break;
                }
            }
            
            debug!("💓 Heartbeat manager stopped");
        })
    }
}

/// Send a heartbeat ping to the client
async fn send_heartbeat(connection: &WebSocketConnection) -> Result<(), anyhow::Error> {
    let heartbeat_msg = json!({
        "type": "heartbeat",
        "timestamp": chrono::Utc::now().timestamp(),
        "message": "ping"
    });
    
    debug!("💓 Sending heartbeat");
    // FIXED: Added the missing detail parameter (None)
    connection.send_status(&heartbeat_msg.to_string(), None).await
}

/// Statistics for heartbeat monitoring
#[derive(Debug, Clone)]
pub struct HeartbeatStats {
    pub heartbeats_sent: u64,
    pub last_heartbeat: Option<std::time::Instant>,
    pub connection_duration: std::time::Duration,
    pub timeouts: u64,
}

impl HeartbeatStats {
    pub fn new() -> Self {
        Self {
            heartbeats_sent: 0,
            last_heartbeat: None,
            connection_duration: std::time::Duration::from_secs(0),
            timeouts: 0,
        }
    }
    
    pub fn record_heartbeat(&mut self) {
        self.heartbeats_sent += 1;
        self.last_heartbeat = Some(std::time::Instant::now());
    }
    
    pub fn record_timeout(&mut self) {
        self.timeouts += 1;
    }
    
    pub fn update_duration(&mut self, duration: std::time::Duration) {
        self.connection_duration = duration;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heartbeat_config() {
        let config = HeartbeatConfig::new(30, 300);
        
        assert_eq!(config.heartbeat_interval, 30);
        assert_eq!(config.connection_timeout, 300);
        assert_eq!(config.processing_heartbeat_interval, 5);
        assert_eq!(config.recent_activity_threshold, 30);
        assert_eq!(config.frequent_heartbeat_interval, 10);
    }
    
    #[test]
    fn test_heartbeat_stats() {
        let mut stats = HeartbeatStats::new();
        
        assert_eq!(stats.heartbeats_sent, 0);
        assert!(stats.last_heartbeat.is_none());
        
        stats.record_heartbeat();
        
        assert_eq!(stats.heartbeats_sent, 1);
        assert!(stats.last_heartbeat.is_some());
        
        stats.record_timeout();
        assert_eq!(stats.timeouts, 1);
    }
}
