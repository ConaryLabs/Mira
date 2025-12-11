// backend/src/mcp/notifications.rs
// Notification handling for MCP servers

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tracing::{debug, info};

/// MCP notification types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method")]
pub enum McpNotification {
    /// Server capabilities changed
    #[serde(rename = "notifications/initialized")]
    Initialized,

    /// Tools list has changed
    #[serde(rename = "notifications/tools/list_changed")]
    ToolsListChanged,

    /// Resources list has changed
    #[serde(rename = "notifications/resources/list_changed")]
    ResourcesListChanged,

    /// Resource content has changed
    #[serde(rename = "notifications/resources/updated")]
    ResourceUpdated {
        #[serde(default)]
        params: Option<ResourceUpdatedParams>,
    },

    /// Prompts list has changed
    #[serde(rename = "notifications/prompts/list_changed")]
    PromptsListChanged,

    /// Progress update
    #[serde(rename = "notifications/progress")]
    Progress {
        #[serde(default)]
        params: Option<ProgressParams>,
    },

    /// Server message/log
    #[serde(rename = "notifications/message")]
    Message {
        #[serde(default)]
        params: Option<MessageParams>,
    },

    /// Cancelled operation
    #[serde(rename = "notifications/cancelled")]
    Cancelled {
        #[serde(default)]
        params: Option<CancelledParams>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceUpdatedParams {
    pub uri: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressParams {
    #[serde(rename = "progressToken")]
    pub progress_token: Value,
    pub progress: f64,
    #[serde(default)]
    pub total: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageParams {
    pub level: String,
    #[serde(default)]
    pub logger: Option<String>,
    pub data: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelledParams {
    #[serde(rename = "requestId")]
    pub request_id: Value,
    #[serde(default)]
    pub reason: Option<String>,
}

/// Handler trait for MCP notifications
#[async_trait]
pub trait NotificationHandler: Send + Sync {
    /// Handle a notification from an MCP server
    async fn handle(&self, server_name: &str, notification: McpNotification);
}

/// Default notification handler that logs and triggers cache refreshes
pub struct DefaultNotificationHandler {
    /// Callback to refresh tools for a server
    on_tools_changed: Option<Arc<dyn Fn(&str) + Send + Sync>>,
    /// Callback to refresh resources for a server
    on_resources_changed: Option<Arc<dyn Fn(&str) + Send + Sync>>,
    /// Callback to refresh prompts for a server
    on_prompts_changed: Option<Arc<dyn Fn(&str) + Send + Sync>>,
}

impl DefaultNotificationHandler {
    pub fn new() -> Self {
        Self {
            on_tools_changed: None,
            on_resources_changed: None,
            on_prompts_changed: None,
        }
    }

    /// Set callback for tools list changes
    pub fn on_tools_changed<F>(mut self, callback: F) -> Self
    where
        F: Fn(&str) + Send + Sync + 'static,
    {
        self.on_tools_changed = Some(Arc::new(callback));
        self
    }

    /// Set callback for resources list changes
    pub fn on_resources_changed<F>(mut self, callback: F) -> Self
    where
        F: Fn(&str) + Send + Sync + 'static,
    {
        self.on_resources_changed = Some(Arc::new(callback));
        self
    }

    /// Set callback for prompts list changes
    pub fn on_prompts_changed<F>(mut self, callback: F) -> Self
    where
        F: Fn(&str) + Send + Sync + 'static,
    {
        self.on_prompts_changed = Some(Arc::new(callback));
        self
    }
}

impl Default for DefaultNotificationHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl NotificationHandler for DefaultNotificationHandler {
    async fn handle(&self, server_name: &str, notification: McpNotification) {
        match notification {
            McpNotification::Initialized => {
                info!("[MCP:{}] Server initialized", server_name);
            }

            McpNotification::ToolsListChanged => {
                info!("[MCP:{}] Tools list changed, refreshing...", server_name);
                if let Some(callback) = &self.on_tools_changed {
                    callback(server_name);
                }
            }

            McpNotification::ResourcesListChanged => {
                info!(
                    "[MCP:{}] Resources list changed, refreshing...",
                    server_name
                );
                if let Some(callback) = &self.on_resources_changed {
                    callback(server_name);
                }
            }

            McpNotification::ResourceUpdated { params } => {
                if let Some(p) = params {
                    info!("[MCP:{}] Resource updated: {}", server_name, p.uri);
                } else {
                    info!("[MCP:{}] Resource updated", server_name);
                }
                // Also trigger resources refresh
                if let Some(callback) = &self.on_resources_changed {
                    callback(server_name);
                }
            }

            McpNotification::PromptsListChanged => {
                info!("[MCP:{}] Prompts list changed, refreshing...", server_name);
                if let Some(callback) = &self.on_prompts_changed {
                    callback(server_name);
                }
            }

            McpNotification::Progress { params } => {
                if let Some(p) = params {
                    let total_str = p
                        .total
                        .map(|t| format!("/{}", t))
                        .unwrap_or_else(|| "".to_string());
                    debug!(
                        "[MCP:{}] Progress: {}{} (token: {:?})",
                        server_name, p.progress, total_str, p.progress_token
                    );
                }
            }

            McpNotification::Message { params } => {
                if let Some(p) = params {
                    let logger = p.logger.as_deref().unwrap_or("default");
                    match p.level.as_str() {
                        "error" => tracing::error!("[MCP:{}:{}] {:?}", server_name, logger, p.data),
                        "warning" => {
                            tracing::warn!("[MCP:{}:{}] {:?}", server_name, logger, p.data)
                        }
                        "info" => tracing::info!("[MCP:{}:{}] {:?}", server_name, logger, p.data),
                        _ => tracing::debug!("[MCP:{}:{}] {:?}", server_name, logger, p.data),
                    }
                }
            }

            McpNotification::Cancelled { params } => {
                if let Some(p) = params {
                    let reason = p.reason.as_deref().unwrap_or("unknown");
                    info!(
                        "[MCP:{}] Request cancelled: {:?} (reason: {})",
                        server_name, p.request_id, reason
                    );
                }
            }
        }
    }
}

/// Parse a JSON-RPC message and extract notification if present
pub fn parse_notification(message: &str) -> Option<McpNotification> {
    let value: Value = serde_json::from_str(message).ok()?;

    // Notifications don't have an "id" field
    if value.get("id").is_some() {
        return None;
    }

    // Must have a method field
    let method = value.get("method")?.as_str()?;

    // Try to parse based on method
    match method {
        "notifications/initialized" => Some(McpNotification::Initialized),
        "notifications/tools/list_changed" => Some(McpNotification::ToolsListChanged),
        "notifications/resources/list_changed" => Some(McpNotification::ResourcesListChanged),
        "notifications/resources/updated" => {
            let params = value
                .get("params")
                .and_then(|p| serde_json::from_value(p.clone()).ok());
            Some(McpNotification::ResourceUpdated { params })
        }
        "notifications/prompts/list_changed" => Some(McpNotification::PromptsListChanged),
        "notifications/progress" => {
            let params = value
                .get("params")
                .and_then(|p| serde_json::from_value(p.clone()).ok());
            Some(McpNotification::Progress { params })
        }
        "notifications/message" => {
            let params = value
                .get("params")
                .and_then(|p| serde_json::from_value(p.clone()).ok());
            Some(McpNotification::Message { params })
        }
        "notifications/cancelled" => {
            let params = value
                .get("params")
                .and_then(|p| serde_json::from_value(p.clone()).ok());
            Some(McpNotification::Cancelled { params })
        }
        _ => {
            debug!("Unknown MCP notification method: {}", method);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_tools_list_changed() {
        let json = r#"{"jsonrpc":"2.0","method":"notifications/tools/list_changed"}"#;
        let notification = parse_notification(json);
        assert!(matches!(
            notification,
            Some(McpNotification::ToolsListChanged)
        ));
    }

    #[test]
    fn test_parse_resources_updated() {
        let json = r#"{"jsonrpc":"2.0","method":"notifications/resources/updated","params":{"uri":"file:///tmp/test.txt"}}"#;
        let notification = parse_notification(json);
        assert!(matches!(
            notification,
            Some(McpNotification::ResourceUpdated { .. })
        ));
        if let Some(McpNotification::ResourceUpdated { params }) = notification {
            assert_eq!(params.unwrap().uri, "file:///tmp/test.txt");
        }
    }

    #[test]
    fn test_parse_progress() {
        let json = r#"{"jsonrpc":"2.0","method":"notifications/progress","params":{"progressToken":"abc123","progress":50.0,"total":100.0}}"#;
        let notification = parse_notification(json);
        assert!(matches!(notification, Some(McpNotification::Progress { .. })));
        if let Some(McpNotification::Progress { params }) = notification {
            let p = params.unwrap();
            assert_eq!(p.progress, 50.0);
            assert_eq!(p.total, Some(100.0));
        }
    }

    #[test]
    fn test_parse_message() {
        let json = r#"{"jsonrpc":"2.0","method":"notifications/message","params":{"level":"info","logger":"test","data":"Hello"}}"#;
        let notification = parse_notification(json);
        assert!(matches!(notification, Some(McpNotification::Message { .. })));
    }

    #[test]
    fn test_ignore_response() {
        // Responses have an id field and should not be parsed as notifications
        let json = r#"{"jsonrpc":"2.0","id":1,"result":{"success":true}}"#;
        let notification = parse_notification(json);
        assert!(notification.is_none());
    }

    #[test]
    fn test_ignore_request() {
        // Requests have an id field and should not be parsed as notifications
        let json = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{}}"#;
        let notification = parse_notification(json);
        assert!(notification.is_none());
    }
}
