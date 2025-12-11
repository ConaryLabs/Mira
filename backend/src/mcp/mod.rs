// backend/src/mcp/mod.rs
// Model Context Protocol (MCP) client implementation
// Enables integration with external MCP servers for tools and resources

pub mod health;
pub mod notifications;
pub mod protocol;
pub mod sampling;
pub mod transport;

pub use health::{HealthMonitor, ServerHealth, TransportConfig};
pub use notifications::{DefaultNotificationHandler, McpNotification, NotificationHandler};
pub use sampling::{
    DenyAllSamplingHandler, SamplingApproval, SamplingApprovalHandler, SamplingRequest,
    SamplingResponse,
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use protocol::{JsonRpcRequest, JsonRpcResponse, McpCapabilities, McpTool};
use transport::{McpTransport, StdioTransport};

/// MCP server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub name: String,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
}

fn default_timeout() -> u64 {
    30000
}

/// MCP configuration file structure
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpConfig {
    #[serde(default)]
    pub servers: Vec<McpServerConfig>,
}

/// Connected MCP server instance
pub struct McpServer {
    pub name: String,
    pub config: McpServerConfig,
    pub capabilities: Option<McpCapabilities>,
    pub tools: Vec<McpTool>,
    pub resources: Vec<protocol::McpResource>,
    pub prompts: Vec<protocol::McpPrompt>,
    transport: Box<dyn McpTransport + Send + Sync>,
    request_id: RwLock<i64>,
}

impl McpServer {
    /// Create a new MCP server connection
    pub async fn connect(config: McpServerConfig) -> Result<Self> {
        let transport: Box<dyn McpTransport + Send + Sync> = if let Some(command) = &config.command
        {
            info!("[MCP] Starting stdio server '{}': {}", config.name, command);
            let stdio = StdioTransport::spawn(command, &config.args, &config.env).await?;
            Box::new(stdio)
        } else if let Some(url) = &config.url {
            info!("[MCP] Connecting to HTTP server '{}': {}", config.name, url);
            let http = transport::HttpTransport::with_timeout(url, config.timeout_ms);
            Box::new(http)
        } else {
            anyhow::bail!("MCP server config must have either 'command' or 'url'");
        };

        let mut server = Self {
            name: config.name.clone(),
            config,
            capabilities: None,
            tools: Vec::new(),
            resources: Vec::new(),
            prompts: Vec::new(),
            transport,
            request_id: RwLock::new(0),
        };

        // Initialize the server
        server.initialize().await?;

        // Discover tools
        server.discover_tools().await?;

        // Discover resources if supported
        server.discover_resources().await?;

        // Discover prompts if supported
        server.discover_prompts().await?;

        Ok(server)
    }

    /// Get next request ID
    async fn next_id(&self) -> i64 {
        let mut id = self.request_id.write().await;
        *id += 1;
        *id
    }

    /// Send a JSON-RPC request and get response
    async fn send_request(&self, method: &str, params: Option<Value>) -> Result<Value> {
        let id = self.next_id().await;
        let request = JsonRpcRequest::new(id, method, params);

        debug!("[MCP:{}] -> {} (id={})", self.name, method, id);

        let request_json = serde_json::to_string(&request)?;
        let response_json = self.transport.send(&request_json).await?;
        let response: JsonRpcResponse = serde_json::from_str(&response_json)?;

        if let Some(error) = response.error {
            anyhow::bail!("MCP error {}: {}", error.code, error.message);
        }

        response.result.context("Empty result from MCP server")
    }

    /// Initialize the MCP connection
    async fn initialize(&mut self) -> Result<()> {
        let params = serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "roots": { "listChanged": true }
            },
            "clientInfo": {
                "name": "mira",
                "version": "1.0.0"
            }
        });

        let result = self.send_request("initialize", Some(params)).await?;

        // Parse capabilities
        if let Ok(caps) = serde_json::from_value::<McpCapabilities>(result.clone()) {
            self.capabilities = Some(caps);
        }

        // Send initialized notification
        let notif = JsonRpcRequest::notification("notifications/initialized", None);
        let notif_json = serde_json::to_string(&notif)?;
        let _ = self.transport.send(&notif_json).await; // Notification, ignore response

        info!(
            "[MCP:{}] Initialized (protocol: {})",
            self.name,
            result
                .get("protocolVersion")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
        );

        Ok(())
    }

    /// Discover available tools from the server
    async fn discover_tools(&mut self) -> Result<()> {
        let result = self.send_request("tools/list", None).await?;

        if let Some(tools) = result.get("tools").and_then(|t| t.as_array()) {
            for tool_value in tools {
                if let Ok(tool) = serde_json::from_value::<McpTool>(tool_value.clone()) {
                    debug!("[MCP:{}] Found tool: {}", self.name, tool.name);
                    self.tools.push(tool);
                }
            }
        }

        info!(
            "[MCP:{}] Discovered {} tools",
            self.name,
            self.tools.len()
        );

        Ok(())
    }

    /// Call a tool on this server
    pub async fn call_tool(&self, tool_name: &str, arguments: Value) -> Result<Value> {
        let params = serde_json::json!({
            "name": tool_name,
            "arguments": arguments
        });

        info!("[MCP:{}] Calling tool: {}", self.name, tool_name);
        let result = self.send_request("tools/call", Some(params)).await?;

        Ok(result)
    }

    /// Discover available resources from the server
    async fn discover_resources(&mut self) -> Result<()> {
        // Check if server supports resources
        let supports_resources = self
            .capabilities
            .as_ref()
            .map(|c| c.resources.is_some())
            .unwrap_or(false);

        if !supports_resources {
            debug!("[MCP:{}] Server does not advertise resource support", self.name);
            return Ok(());
        }

        let result = self.send_request("resources/list", None).await?;

        if let Some(resources) = result.get("resources").and_then(|r| r.as_array()) {
            for resource_value in resources {
                if let Ok(resource) =
                    serde_json::from_value::<protocol::McpResource>(resource_value.clone())
                {
                    debug!("[MCP:{}] Found resource: {}", self.name, resource.uri);
                    self.resources.push(resource);
                }
            }
        }

        info!(
            "[MCP:{}] Discovered {} resources",
            self.name,
            self.resources.len()
        );

        Ok(())
    }

    /// Discover available prompts from the server
    async fn discover_prompts(&mut self) -> Result<()> {
        // Check if server supports prompts
        let supports_prompts = self
            .capabilities
            .as_ref()
            .map(|c| c.prompts.is_some())
            .unwrap_or(false);

        if !supports_prompts {
            debug!("[MCP:{}] Server does not advertise prompt support", self.name);
            return Ok(());
        }

        let result = self.send_request("prompts/list", None).await?;

        if let Some(prompts) = result.get("prompts").and_then(|p| p.as_array()) {
            for prompt_value in prompts {
                if let Ok(prompt) =
                    serde_json::from_value::<protocol::McpPrompt>(prompt_value.clone())
                {
                    debug!("[MCP:{}] Found prompt: {}", self.name, prompt.name);
                    self.prompts.push(prompt);
                }
            }
        }

        info!(
            "[MCP:{}] Discovered {} prompts",
            self.name,
            self.prompts.len()
        );

        Ok(())
    }

    /// List available resources
    pub async fn list_resources(&self) -> Result<Vec<protocol::McpResource>> {
        Ok(self.resources.clone())
    }

    /// Read a resource by URI
    pub async fn read_resource(&self, uri: &str) -> Result<protocol::ResourceReadResult> {
        let params = serde_json::json!({
            "uri": uri
        });

        let result = self.send_request("resources/read", Some(params)).await?;
        serde_json::from_value(result).context("Failed to parse resource read result")
    }

    /// Subscribe to resource changes (if supported)
    pub async fn subscribe_resource(&self, uri: &str) -> Result<()> {
        let supports_subscribe = self
            .capabilities
            .as_ref()
            .and_then(|c| c.resources.as_ref())
            .map(|r| r.subscribe)
            .unwrap_or(false);

        if !supports_subscribe {
            anyhow::bail!("Server '{}' does not support resource subscriptions", self.name);
        }

        let params = serde_json::json!({ "uri": uri });
        self.send_request("resources/subscribe", Some(params)).await?;
        info!("[MCP:{}] Subscribed to resource: {}", self.name, uri);
        Ok(())
    }

    /// Unsubscribe from resource changes
    pub async fn unsubscribe_resource(&self, uri: &str) -> Result<()> {
        let params = serde_json::json!({ "uri": uri });
        self.send_request("resources/unsubscribe", Some(params)).await?;
        info!("[MCP:{}] Unsubscribed from resource: {}", self.name, uri);
        Ok(())
    }

    /// List available prompts
    pub async fn list_prompts(&self) -> Result<Vec<protocol::McpPrompt>> {
        Ok(self.prompts.clone())
    }

    /// Get a prompt with resolved arguments
    pub async fn get_prompt(
        &self,
        name: &str,
        arguments: std::collections::HashMap<String, String>,
    ) -> Result<Value> {
        let params = serde_json::json!({
            "name": name,
            "arguments": arguments
        });

        self.send_request("prompts/get", Some(params)).await
    }
}

/// MCP Manager - manages multiple MCP server connections
pub struct McpManager {
    servers: RwLock<HashMap<String, Arc<McpServer>>>,
    config: RwLock<McpConfig>,
    health_monitor: Arc<HealthMonitor>,
    transport_config: TransportConfig,
}

impl McpManager {
    pub fn new() -> Self {
        let transport_config = TransportConfig::from_env();
        Self {
            servers: RwLock::new(HashMap::new()),
            config: RwLock::new(McpConfig::default()),
            health_monitor: Arc::new(HealthMonitor::new(transport_config.health_check_interval_ms)),
            transport_config,
        }
    }

    /// Create with custom transport configuration
    pub fn with_config(transport_config: TransportConfig) -> Self {
        Self {
            servers: RwLock::new(HashMap::new()),
            config: RwLock::new(McpConfig::default()),
            health_monitor: Arc::new(HealthMonitor::new(transport_config.health_check_interval_ms)),
            transport_config,
        }
    }

    /// Get the health monitor
    pub fn health_monitor(&self) -> &Arc<HealthMonitor> {
        &self.health_monitor
    }

    /// Get health status for all servers
    pub async fn get_all_health(&self) -> Vec<ServerHealth> {
        self.health_monitor.all_health().await
    }

    /// Check if a server is healthy
    pub async fn is_server_healthy(&self, server_name: &str) -> bool {
        self.health_monitor.is_healthy(server_name).await
    }

    /// Load MCP configuration from file
    pub async fn load_config(&self, config_path: Option<&Path>) -> Result<()> {
        // Try project config first, then user config
        let paths = if let Some(path) = config_path {
            vec![path.to_path_buf()]
        } else {
            let mut paths = vec![];

            // Project config
            if let Ok(cwd) = std::env::current_dir() {
                paths.push(cwd.join(".mira/mcp.json"));
            }

            // User config
            if let Some(home) = dirs::home_dir() {
                paths.push(home.join(".mira/mcp.json"));
            }

            paths
        };

        for path in paths {
            if path.exists() {
                info!("[MCP] Loading config from {:?}", path);
                let content = tokio::fs::read_to_string(&path).await?;
                let config: McpConfig = serde_json::from_str(&content)
                    .context("Failed to parse MCP config")?;

                let mut cfg = self.config.write().await;
                *cfg = config;

                return Ok(());
            }
        }

        debug!("[MCP] No config file found, using defaults");
        Ok(())
    }

    /// Connect to all configured servers
    pub async fn connect_all(&self) -> Result<()> {
        let config = self.config.read().await.clone();

        for server_config in config.servers {
            let name = server_config.name.clone();
            match McpServer::connect(server_config).await {
                Ok(server) => {
                    let mut servers = self.servers.write().await;
                    servers.insert(name.clone(), Arc::new(server));
                    // Register with health monitor
                    self.health_monitor.register_server(&name).await;
                    info!("[MCP] Connected to server '{}'", name);
                }
                Err(e) => {
                    warn!("[MCP] Failed to connect to '{}': {}", name, e);
                }
            }
        }

        Ok(())
    }

    /// Get all available tools from all servers
    pub async fn get_all_tools(&self) -> Vec<(String, McpTool)> {
        let servers = self.servers.read().await;
        let mut all_tools = Vec::new();

        for (server_name, server) in servers.iter() {
            for tool in &server.tools {
                all_tools.push((server_name.clone(), tool.clone()));
            }
        }

        all_tools
    }

    /// Call a tool on the appropriate server with health tracking
    pub async fn call_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        arguments: Value,
    ) -> Result<Value> {
        let servers = self.servers.read().await;
        let server = servers
            .get(server_name)
            .context(format!("MCP server '{}' not found", server_name))?;

        // Execute tool call and track health
        match server.call_tool(tool_name, arguments).await {
            Ok(result) => {
                self.health_monitor.record_success(server_name).await;
                Ok(result)
            }
            Err(e) => {
                self.health_monitor
                    .record_failure(server_name, &e.to_string())
                    .await;
                Err(e)
            }
        }
    }

    /// Get connected server count
    pub async fn server_count(&self) -> usize {
        self.servers.read().await.len()
    }

    /// List connected servers
    pub async fn list_servers(&self) -> Vec<String> {
        self.servers.read().await.keys().cloned().collect()
    }

    /// Get all available resources from all servers
    pub async fn get_all_resources(&self) -> Vec<(String, protocol::McpResource)> {
        let servers = self.servers.read().await;
        let mut all_resources = Vec::new();

        for (server_name, server) in servers.iter() {
            for resource in &server.resources {
                all_resources.push((server_name.clone(), resource.clone()));
            }
        }

        all_resources
    }

    /// Get all available prompts from all servers
    pub async fn get_all_prompts(&self) -> Vec<(String, protocol::McpPrompt)> {
        let servers = self.servers.read().await;
        let mut all_prompts = Vec::new();

        for (server_name, server) in servers.iter() {
            for prompt in &server.prompts {
                all_prompts.push((server_name.clone(), prompt.clone()));
            }
        }

        all_prompts
    }

    /// Read a resource from a specific server
    pub async fn read_resource(&self, server_name: &str, uri: &str) -> Result<protocol::ResourceReadResult> {
        let servers = self.servers.read().await;
        let server = servers
            .get(server_name)
            .context(format!("MCP server '{}' not found", server_name))?;

        server.read_resource(uri).await
    }

    /// Get a prompt from a specific server
    pub async fn get_prompt(
        &self,
        server_name: &str,
        prompt_name: &str,
        arguments: std::collections::HashMap<String, String>,
    ) -> Result<Value> {
        let servers = self.servers.read().await;
        let server = servers
            .get(server_name)
            .context(format!("MCP server '{}' not found", server_name))?;

        server.get_prompt(prompt_name, arguments).await
    }
}

impl Default for McpManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_mcp_config() {
        let json = r#"{
            "servers": [
                {
                    "name": "filesystem",
                    "command": "npx",
                    "args": ["-y", "@anthropic/mcp-server-filesystem"],
                    "env": {"HOME": "/home/user"}
                }
            ]
        }"#;

        let config: McpConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.servers.len(), 1);
        assert_eq!(config.servers[0].name, "filesystem");
        assert_eq!(config.servers[0].command, Some("npx".to_string()));
        assert_eq!(config.servers[0].args.len(), 2);
    }

    #[test]
    fn test_default_config() {
        let config = McpConfig::default();
        assert!(config.servers.is_empty());
    }

    #[tokio::test]
    async fn test_mcp_manager_creation() {
        let manager = McpManager::new();
        assert_eq!(manager.server_count().await, 0);
    }
}
