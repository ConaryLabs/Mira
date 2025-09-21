// src/api/ws/chat/heartbeat.rs
// Heartbeat management for WebSocket connections

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use tokio::task::JoinHandle;
use tokio::time::interval;
use tracing::{debug, warn};

use super::connection::WebSocketConnection;

#[derive(Debug, Clone)]
pub struct HeartbeatConfig {
    pub heartbeat_interval: u64,
    pub connection_timeout: u64,
    pub processing_heartbeat_interval: u64,
    pub recent_activity_threshold: u64,
    pub frequent_heartbeat_interval: u64,
}

impl HeartbeatConfig {
    pub fn from_defaults() -> Self {
        Self {
            heartbeat_interval: 30,
            connection_timeout: 600,
            processing_heartbeat_interval: 5,
            recent_activity_threshold: 30,
            frequent_heartbeat_interval: 10,
        }
    }

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
            config: HeartbeatConfig::from_defaults(),
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

    pub fn start(&self) -> JoinHandle<()> {
        let connection = self.connection.clone();
        let config = self.config.clone();

        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(config.heartbeat_interval));
            
            debug!("Heartbeat manager started with interval: {}s", config.heartbeat_interval);
            
            loop {
                ticker.tick().await;
                
                if connection.is_processing().await {
                    debug!("Connection is processing, using quick heartbeat");
                    tokio::time::sleep(Duration::from_secs(config.processing_heartbeat_interval)).await;
                    continue;
                }
                
                let last_activity = connection.get_last_activity().await;
                let time_since_activity = last_activity.elapsed();
                
                if time_since_activity.as_secs() < config.recent_activity_threshold {
                    debug!("Recent activity detected, using frequent heartbeat");
                    tokio::time::sleep(Duration::from_secs(config.frequent_heartbeat_interval)).await;
                    continue;
                }
                
                if let Err(e) = send_heartbeat(&connection).await {
                    warn!("Failed to send heartbeat: {}", e);
                    break;
                }
                
                let last_send = connection.get_last_send().await;
                let time_since_send = last_send.elapsed();
                
                if time_since_send.as_secs() > config.connection_timeout {
                    warn!("Connection timeout after {}s", time_since_send.as_secs());
                    break;
                }
            }
            
            debug!("Heartbeat manager stopped");
        })
    }
}

async fn send_heartbeat(connection: &WebSocketConnection) -> Result<(), anyhow::Error> {
    let heartbeat_msg = json!({
        "type": "heartbeat",
        "timestamp": chrono::Utc::now().timestamp(),
        "message": "ping"
    });
    
    debug!("Sending heartbeat");
    connection.send_status(&heartbeat_msg.to_string(), None).await
}

#[derive(Debug, Clone)]
pub struct HeartbeatStats {
    pub heartbeats_sent: u64,
    pub last_heartbeat: Option<std::time::Instant>,
    pub connection_duration: std::time::Duration,
    pub timeouts: u64,
}

impl Default for HeartbeatStats {
    fn default() -> Self {
        Self::new()
    }
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
