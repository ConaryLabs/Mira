// src/api/ws/heartbeat.rs
// Phase 3: Extract Heartbeat Management from chat.rs
// Handles dynamic heartbeat intervals, connection timeouts, and ping/pong logic

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use tokio::task::JoinHandle;
use tokio::time::interval;
use tracing::{debug, warn};

use crate::api::ws::connection::WebSocketConnection;
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
            
            debug!("💓 Heartbeat task started with interval: {}s, timeout: {}s", 
                   config.heartbeat_interval, config.connection_timeout);

            loop {
                ticker.tick().await;
                
                // Get current state
                let activity_elapsed = connection.get_last_activity().await.elapsed();
                let send_elapsed = connection.get_last_send().await.elapsed();
                let processing = connection.is_processing().await;
                
                // Dynamic heartbeat interval based on activity and processing state
                let should_send = if processing {
                    // Send heartbeat every 5 seconds when processing
                    send_elapsed > Duration::from_secs(config.processing_heartbeat_interval)
                } else if activity_elapsed < Duration::from_secs(config.recent_activity_threshold) {
                    // Send heartbeat every 10 seconds for recently active connections
                    send_elapsed > Duration::from_secs(config.frequent_heartbeat_interval)
                } else {
                    // Standard heartbeat interval for idle connections
                    send_elapsed > Duration::from_secs(config.heartbeat_interval)
                };
                
                // Send heartbeat if needed
                if should_send {
                    if let Err(e) = Self::send_ping(&connection).await {
                        debug!("💓 Failed to send heartbeat: {}, ending task", e);
                        break;
                    }
                }
                
                // Check for connection timeout
                if activity_elapsed > Duration::from_secs(config.connection_timeout) && !processing {
                    warn!("⏱️ Connection timeout after {:?} of inactivity", activity_elapsed);
                    break;
                }
            }
            
            debug!("💓 Heartbeat task ended");
        })
    }

    /// Send a ping message to the client
    async fn send_ping(connection: &WebSocketConnection) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let ping_msg = json!({
            "type": "ping",
            "ts": chrono::Utc::now().timestamp_millis()
        });
        
        debug!("💓 Sending heartbeat ping");
        
        // Use try_lock to avoid blocking the heartbeat task
        if let Ok(mut lock) = connection.get_sender().try_lock() {
            use futures_util::SinkExt;
            use axum::extract::ws::Message;
            
            lock.send(Message::Text(ping_msg.to_string())).await?;
            
            // Update last send time
            connection.update_activity().await;
            
            Ok(())
        } else {
            // Sender is busy, skip this heartbeat
            debug!("💓 Sender busy, skipping heartbeat");
            Ok(())
        }
    }

    /// Check if the connection has timed out
    pub async fn is_timed_out(&self) -> bool {
        let activity_elapsed = self.connection.get_last_activity().await.elapsed();
        let processing = self.connection.is_processing().await;
        
        activity_elapsed > Duration::from_secs(self.config.connection_timeout) && !processing
    }

    /// Get time until connection timeout
    pub async fn time_until_timeout(&self) -> Option<Duration> {
        let activity_elapsed = self.connection.get_last_activity().await.elapsed();
        let timeout_duration = Duration::from_secs(self.config.connection_timeout);
        
        if activity_elapsed >= timeout_duration {
            None
        } else {
            Some(timeout_duration - activity_elapsed)
        }
    }

    /// Get heartbeat statistics for debugging
    pub async fn get_stats(&self) -> HeartbeatStats {
        HeartbeatStats {
            last_activity_elapsed: self.connection.get_last_activity().await.elapsed(),
            last_send_elapsed: self.connection.get_last_send().await.elapsed(),
            is_processing: self.connection.is_processing().await,
            config: self.config.clone(),
        }
    }
}

#[derive(Debug)]
pub struct HeartbeatStats {
    pub last_activity_elapsed: Duration,
    pub last_send_elapsed: Duration,
    pub is_processing: bool,
    pub config: HeartbeatConfig,
}

impl HeartbeatStats {
    /// Get the next expected heartbeat interval based on current state
    pub fn next_heartbeat_interval(&self) -> Duration {
        if self.is_processing {
            Duration::from_secs(self.config.processing_heartbeat_interval)
        } else if self.last_activity_elapsed < Duration::from_secs(self.config.recent_activity_threshold) {
            Duration::from_secs(self.config.frequent_heartbeat_interval)
        } else {
            Duration::from_secs(self.config.heartbeat_interval)
        }
    }

    /// Check if a heartbeat should be sent now
    pub fn should_send_heartbeat(&self) -> bool {
        let next_interval = self.next_heartbeat_interval();
        self.last_send_elapsed >= next_interval
    }

    /// Check if the connection should timeout
    pub fn should_timeout(&self) -> bool {
        self.last_activity_elapsed > Duration::from_secs(self.config.connection_timeout) 
            && !self.is_processing
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_heartbeat_config() {
        let config = HeartbeatConfig::new(25, 180);
        
        assert_eq!(config.heartbeat_interval, 25);
        assert_eq!(config.connection_timeout, 180);
        assert_eq!(config.processing_heartbeat_interval, 5);
        assert_eq!(config.recent_activity_threshold, 30);
        assert_eq!(config.frequent_heartbeat_interval, 10);
    }

    #[test]
    fn test_heartbeat_stats_intervals() {
        let config = HeartbeatConfig::new(25, 180);
        
        // Test processing state
        let stats = HeartbeatStats {
            last_activity_elapsed: Duration::from_secs(10),
            last_send_elapsed: Duration::from_secs(3),
            is_processing: true,
            config: config.clone(),
        };
        
        assert_eq!(stats.next_heartbeat_interval(), Duration::from_secs(5));
        assert!(!stats.should_send_heartbeat()); // Only 3 seconds elapsed
        assert!(!stats.should_timeout()); // Processing
        
        // Test recent activity state
        let stats = HeartbeatStats {
            last_activity_elapsed: Duration::from_secs(20), // Recent
            last_send_elapsed: Duration::from_secs(11), // Over frequent interval
            is_processing: false,
            config: config.clone(),
        };
        
        assert_eq!(stats.next_heartbeat_interval(), Duration::from_secs(10));
        assert!(stats.should_send_heartbeat());
        assert!(!stats.should_timeout());
        
        // Test idle state
        let stats = HeartbeatStats {
            last_activity_elapsed: Duration::from_secs(40), // Old activity
            last_send_elapsed: Duration::from_secs(26), // Over base interval
            is_processing: false,
            config: config.clone(),
        };
        
        assert_eq!(stats.next_heartbeat_interval(), Duration::from_secs(25));
        assert!(stats.should_send_heartbeat());
        assert!(!stats.should_timeout());
        
        // Test timeout state
        let stats = HeartbeatStats {
            last_activity_elapsed: Duration::from_secs(200), // Way over timeout
            last_send_elapsed: Duration::from_secs(26),
            is_processing: false,
            config: config.clone(),
        };
        
        assert!(stats.should_timeout());
    }

    // Note: Testing the actual HeartbeatManager requires a WebSocketConnection,
    // which requires a real WebSocket. Integration tests would be better for this.
}
