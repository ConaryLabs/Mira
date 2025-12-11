// backend/src/mcp/health.rs
// Health monitoring for MCP server connections

use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, warn};

/// Health status for an MCP server
#[derive(Debug, Clone)]
pub struct ServerHealth {
    pub name: String,
    pub connected: bool,
    pub last_success: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub consecutive_failures: u32,
    pub total_requests: u64,
    pub total_failures: u64,
}

impl ServerHealth {
    pub fn new(name: String) -> Self {
        Self {
            name,
            connected: true,
            last_success: Some(Utc::now()),
            last_error: None,
            consecutive_failures: 0,
            total_requests: 0,
            total_failures: 0,
        }
    }

    pub fn record_success(&mut self) {
        self.connected = true;
        self.last_success = Some(Utc::now());
        self.consecutive_failures = 0;
        self.total_requests += 1;
    }

    pub fn record_failure(&mut self, error: &str) {
        self.consecutive_failures += 1;
        self.total_failures += 1;
        self.total_requests += 1;
        self.last_error = Some(error.to_string());

        // Mark as disconnected after 3 consecutive failures
        if self.consecutive_failures >= 3 {
            self.connected = false;
        }
    }

    pub fn success_rate(&self) -> f64 {
        if self.total_requests == 0 {
            return 1.0;
        }
        (self.total_requests - self.total_failures) as f64 / self.total_requests as f64
    }
}

/// Health monitor for all MCP servers
pub struct HealthMonitor {
    servers: Arc<RwLock<HashMap<String, ServerHealth>>>,
    check_interval_ms: u64,
}

impl HealthMonitor {
    pub fn new(check_interval_ms: u64) -> Self {
        Self {
            servers: Arc::new(RwLock::new(HashMap::new())),
            check_interval_ms,
        }
    }

    /// Register a server for health tracking
    pub async fn register_server(&self, name: &str) {
        let mut servers = self.servers.write().await;
        servers.insert(name.to_string(), ServerHealth::new(name.to_string()));
        debug!("[HealthMonitor] Registered server: {}", name);
    }

    /// Unregister a server
    pub async fn unregister_server(&self, name: &str) {
        let mut servers = self.servers.write().await;
        servers.remove(name);
        debug!("[HealthMonitor] Unregistered server: {}", name);
    }

    /// Record a successful request
    pub async fn record_success(&self, server_name: &str) {
        let mut servers = self.servers.write().await;
        if let Some(health) = servers.get_mut(server_name) {
            health.record_success();
        }
    }

    /// Record a failed request
    pub async fn record_failure(&self, server_name: &str, error: &str) {
        let mut servers = self.servers.write().await;
        if let Some(health) = servers.get_mut(server_name) {
            health.record_failure(error);
            if health.consecutive_failures >= 3 {
                warn!(
                    "[HealthMonitor] Server '{}' marked unhealthy after {} consecutive failures",
                    server_name, health.consecutive_failures
                );
            }
        }
    }

    /// Get health status for a specific server
    pub async fn get_health(&self, server_name: &str) -> Option<ServerHealth> {
        self.servers.read().await.get(server_name).cloned()
    }

    /// Get health status for all servers
    pub async fn all_health(&self) -> Vec<ServerHealth> {
        self.servers.read().await.values().cloned().collect()
    }

    /// Check if a server is healthy (connected and responding)
    pub async fn is_healthy(&self, server_name: &str) -> bool {
        self.servers
            .read()
            .await
            .get(server_name)
            .map(|h| h.connected)
            .unwrap_or(false)
    }

    /// Get check interval in milliseconds
    pub fn check_interval_ms(&self) -> u64 {
        self.check_interval_ms
    }
}

impl Default for HealthMonitor {
    fn default() -> Self {
        Self::new(30_000) // 30 second default
    }
}

/// Transport configuration for resilience
#[derive(Debug, Clone)]
pub struct TransportConfig {
    /// Connection timeout in milliseconds
    pub connect_timeout_ms: u64,
    /// Request timeout in milliseconds
    pub request_timeout_ms: u64,
    /// Maximum retry attempts
    pub max_retries: u32,
    /// Backoff between retries in milliseconds
    pub retry_backoff_ms: u64,
    /// Health check interval in milliseconds
    pub health_check_interval_ms: u64,
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            connect_timeout_ms: 30_000,
            request_timeout_ms: 30_000,
            max_retries: 3,
            retry_backoff_ms: 1_000,
            health_check_interval_ms: 30_000,
        }
    }
}

impl TransportConfig {
    /// Create config from environment variables with defaults
    pub fn from_env() -> Self {
        Self {
            connect_timeout_ms: std::env::var("MCP_CONNECT_TIMEOUT_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(30_000),
            request_timeout_ms: std::env::var("MCP_REQUEST_TIMEOUT_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(30_000),
            max_retries: std::env::var("MCP_MAX_RETRIES")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3),
            retry_backoff_ms: std::env::var("MCP_RETRY_BACKOFF_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1_000),
            health_check_interval_ms: std::env::var("MCP_HEALTH_CHECK_INTERVAL_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(30_000),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_health_monitor_registration() {
        let monitor = HealthMonitor::new(30_000);
        monitor.register_server("test-server").await;

        let health = monitor.get_health("test-server").await;
        assert!(health.is_some());
        assert!(health.unwrap().connected);
    }

    #[tokio::test]
    async fn test_health_monitor_success_tracking() {
        let monitor = HealthMonitor::new(30_000);
        monitor.register_server("test-server").await;

        monitor.record_success("test-server").await;
        monitor.record_success("test-server").await;

        let health = monitor.get_health("test-server").await.unwrap();
        assert_eq!(health.total_requests, 2);
        assert_eq!(health.total_failures, 0);
        assert!(health.connected);
    }

    #[tokio::test]
    async fn test_health_monitor_failure_tracking() {
        let monitor = HealthMonitor::new(30_000);
        monitor.register_server("test-server").await;

        // After 3 consecutive failures, server should be marked unhealthy
        monitor.record_failure("test-server", "error 1").await;
        assert!(monitor.is_healthy("test-server").await);

        monitor.record_failure("test-server", "error 2").await;
        assert!(monitor.is_healthy("test-server").await);

        monitor.record_failure("test-server", "error 3").await;
        assert!(!monitor.is_healthy("test-server").await);

        let health = monitor.get_health("test-server").await.unwrap();
        assert_eq!(health.consecutive_failures, 3);
        assert!(!health.connected);
    }

    #[tokio::test]
    async fn test_health_recovery_after_success() {
        let monitor = HealthMonitor::new(30_000);
        monitor.register_server("test-server").await;

        // Fail 3 times
        for _ in 0..3 {
            monitor.record_failure("test-server", "error").await;
        }
        assert!(!monitor.is_healthy("test-server").await);

        // One success should recover
        monitor.record_success("test-server").await;
        assert!(monitor.is_healthy("test-server").await);
    }

    #[test]
    fn test_transport_config_default() {
        let config = TransportConfig::default();
        assert_eq!(config.connect_timeout_ms, 30_000);
        assert_eq!(config.max_retries, 3);
    }

    #[test]
    fn test_success_rate_calculation() {
        let mut health = ServerHealth::new("test".to_string());

        // 10 requests, 2 failures
        for _ in 0..8 {
            health.record_success();
        }
        for _ in 0..2 {
            health.record_failure("error");
        }

        assert!((health.success_rate() - 0.8).abs() < 0.01);
    }
}
