// backend/src/mcp/mod.rs
// Model Context Protocol (MCP) client implementation
// Enables integration with external MCP servers for tools and resources

pub mod protocol;
pub mod transport;

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
        } else if let Some(_url) = &config.url {
            // HTTP transport would go here
            anyhow::bail!("HTTP transport not yet implemented");
        } else {
            anyhow::bail!("MCP server config must have either 'command' or 'url'");
        };

        let mut server = Self {
            name: config.name.clone(),
            config,
            capabilities: None,
            tools: Vec::new(),
            transport,
            request_id: RwLock::new(0),
        };

        // Initialize the server
        server.initialize().await?;

        // Discover tools
        server.discover_tools().await?;

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
                "version": "0.9.0"
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

    /// List available resources
    pub async fn list_resources(&self) -> Result<Vec<Value>> {
        let result = self.send_request("resources/list", None).await?;

        if let Some(resources) = result.get("resources").and_then(|r| r.as_array()) {
            Ok(resources.clone())
        } else {
            Ok(Vec::new())
        }
    }

    /// Read a resource
    pub async fn read_resource(&self, uri: &str) -> Result<Value> {
        let params = serde_json::json!({
            "uri": uri
        });

        self.send_request("resources/read", Some(params)).await
    }
}

/// MCP Manager - manages multiple MCP server connections
pub struct McpManager {
    servers: RwLock<HashMap<String, Arc<McpServer>>>,
    config: RwLock<McpConfig>,
}

impl McpManager {
    pub fn new() -> Self {
        Self {
            servers: RwLock::new(HashMap::new()),
            config: RwLock::new(McpConfig::default()),
        }
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

    /// Call a tool on the appropriate server
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

        server.call_tool(tool_name, arguments).await
    }

    /// Get connected server count
    pub async fn server_count(&self) -> usize {
        self.servers.read().await.len()
    }

    /// List connected servers
    pub async fn list_servers(&self) -> Vec<String> {
        self.servers.read().await.keys().cloned().collect()
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
