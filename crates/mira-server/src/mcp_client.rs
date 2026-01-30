// crates/mira-server/src/mcp_client.rs
// MCP Client Manager - connects to external MCP servers for expert tool access

use crate::llm::Tool;
use crate::tools::core::McpToolInfo;
use rmcp::model::{CallToolRequestParam, CallToolResult, ClientInfo};
use rmcp::service::{Peer, RunningService};
use rmcp::transport::child_process::TokioChildProcess;
use rmcp::{RoleClient, serve_client};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use tokio::process::Command;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Configuration for an external MCP server
#[derive(Debug, Clone)]
pub struct McpServerConfig {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
}

/// A connected MCP server with its peer handle
struct ConnectedServer {
    peer: Peer<RoleClient>,
    /// Cached tool list from this server
    tools: Vec<rmcp::model::Tool>,
    /// Keep the RunningService alive to prevent transport shutdown.
    /// Dropping this cancels the transport and kills the child process.
    _service: RunningService<RoleClient, ClientInfo>,
}

/// Manages connections to external MCP servers
pub struct McpClientManager {
    configs: Vec<McpServerConfig>,
    clients: Arc<RwLock<HashMap<String, ConnectedServer>>>,
}

impl McpClientManager {
    /// Create a new manager from MCP config files
    /// Reads .mcp.json from project path and ~/.claude/mcp.json (global)
    pub fn from_mcp_configs(project_path: Option<&str>) -> Self {
        let mut configs = Vec::new();
        let mut seen_names = std::collections::HashSet::new();

        // Read project-level .mcp.json first (takes precedence)
        if let Some(path) = project_path {
            let project_mcp = format!("{}/.mcp.json", path);
            if let Ok(content) = std::fs::read_to_string(&project_mcp) {
                if let Ok(parsed) = serde_json::from_str::<Value>(&content) {
                    Self::parse_mcp_servers(&parsed, &mut configs, &mut seen_names);
                }
            }
        }

        // Read global ~/.claude/mcp.json
        if let Some(home) = dirs::home_dir() {
            let global_mcp = home.join(".claude/mcp.json");
            if let Ok(content) = std::fs::read_to_string(&global_mcp) {
                if let Ok(parsed) = serde_json::from_str::<Value>(&content) {
                    Self::parse_mcp_servers(&parsed, &mut configs, &mut seen_names);
                }
            }
        }

        if configs.is_empty() {
            debug!("No external MCP servers found in config files");
        } else {
            info!(
                count = configs.len(),
                servers = ?configs.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
                "Loaded MCP server configs"
            );
        }

        Self {
            configs,
            clients: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Parse mcpServers from a JSON config object
    fn parse_mcp_servers(
        config: &Value,
        configs: &mut Vec<McpServerConfig>,
        seen: &mut std::collections::HashSet<String>,
    ) {
        let servers = match config.get("mcpServers").and_then(|v| v.as_object()) {
            Some(s) => s,
            None => return,
        };

        for (name, server_config) in servers {
            // Skip "mira" (ourselves)
            if name == "mira" {
                continue;
            }

            // Skip if already seen (project overrides global)
            if seen.contains(name) {
                continue;
            }

            let command = match server_config.get("command").and_then(|v| v.as_str()) {
                Some(c) => c.to_string(),
                None => continue,
            };

            let args: Vec<String> = server_config
                .get("args")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str())
                        .map(String::from)
                        .collect()
                })
                .unwrap_or_default();

            let env: HashMap<String, String> = server_config
                .get("env")
                .and_then(|v| v.as_object())
                .map(|obj| {
                    obj.iter()
                        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                        .collect()
                })
                .unwrap_or_default();

            seen.insert(name.clone());
            configs.push(McpServerConfig {
                name: name.clone(),
                command,
                args,
                env,
            });
        }
    }

    /// Ensure a server is connected, lazily connecting if needed
    async fn ensure_connected(&self, server_name: &str) -> Result<(), String> {
        // Fast path: already connected
        {
            let clients = self.clients.read().await;
            if clients.contains_key(server_name) {
                return Ok(());
            }
        }

        // Find the config
        let config = self
            .configs
            .iter()
            .find(|c| c.name == server_name)
            .ok_or_else(|| format!("MCP server '{}' not configured", server_name))?
            .clone();

        // Security: log the full command being spawned so users can audit .mcp.json behavior
        let env_keys: Vec<&str> = config.env.keys().map(|k| k.as_str()).collect();
        warn!(
            server = %server_name,
            command = %config.command,
            args = ?config.args,
            env_vars = ?env_keys,
            "Spawning MCP server child process from .mcp.json"
        );

        // Build the command
        let mut cmd = Command::new(&config.command);
        cmd.args(&config.args);
        for (key, value) in &config.env {
            cmd.env(key, value);
        }
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null()); // Suppress server stderr

        // Spawn the child process transport
        let transport = TokioChildProcess::new(cmd)
            .map_err(|e| format!("Failed to spawn MCP server '{}': {}", server_name, e))?;

        // Connect as MCP client
        let client_info = ClientInfo {
            protocol_version: Default::default(),
            capabilities: Default::default(),
            client_info: rmcp::model::Implementation {
                name: "mira-expert".into(),
                title: Some("Mira Expert Sub-Agent".into()),
                version: env!("CARGO_PKG_VERSION").into(),
                icons: None,
                website_url: None,
            },
        };

        let service = serve_client(client_info, transport).await.map_err(|e| {
            format!(
                "Failed to initialize MCP client for '{}': {}",
                server_name, e
            )
        })?;

        let peer = service.peer().clone();

        // List tools from this server
        let tools = peer
            .list_all_tools()
            .await
            .map_err(|e| format!("Failed to list tools from '{}': {}", server_name, e))?;

        info!(
            server = %server_name,
            tool_count = tools.len(),
            "Connected to MCP server"
        );

        // Store the connected server (keeping service alive to maintain transport)
        let mut clients = self.clients.write().await;
        clients.insert(
            server_name.to_string(),
            ConnectedServer {
                peer,
                tools,
                _service: service,
            },
        );

        Ok(())
    }

    /// List tools from all configured MCP servers
    /// Returns Vec of (server_name, tools) pairs
    pub async fn list_tools(&self) -> Vec<(String, Vec<McpToolInfo>)> {
        let mut result = Vec::new();

        for config in &self.configs {
            if let Err(e) = self.ensure_connected(&config.name).await {
                warn!(server = %config.name, error = %e, "Failed to connect to MCP server");
                continue;
            }

            let clients = self.clients.read().await;
            if let Some(server) = clients.get(&config.name) {
                let tools: Vec<McpToolInfo> = server
                    .tools
                    .iter()
                    .map(|t| McpToolInfo {
                        name: t.name.to_string(),
                        description: t.description.as_deref().unwrap_or("").to_string(),
                    })
                    .collect();

                if !tools.is_empty() {
                    result.push((config.name.clone(), tools));
                }
            }
        }

        result
    }

    /// Get tools formatted for LLM consumption (as expert Tool definitions)
    /// Tool names are prefixed with mcp__{server}__{tool_name}
    pub async fn get_expert_tools(&self) -> Vec<Tool> {
        let mut tools = Vec::new();

        for config in &self.configs {
            if let Err(e) = self.ensure_connected(&config.name).await {
                warn!(server = %config.name, error = %e, "Failed to connect to MCP server for tools");
                continue;
            }

            let clients = self.clients.read().await;
            if let Some(server) = clients.get(&config.name) {
                for mcp_tool in &server.tools {
                    let prefixed_name = format!("mcp__{}__{}", config.name, mcp_tool.name);
                    let description = mcp_tool
                        .description
                        .as_deref()
                        .unwrap_or("No description")
                        .to_string();

                    // Convert MCP tool input_schema (Arc<JsonObject>) to our Tool format
                    let parameters = serde_json::to_value(mcp_tool.input_schema.as_ref())
                        .unwrap_or(json!({"type": "object", "properties": {}}));

                    tools.push(Tool::function(prefixed_name, description, parameters));
                }
            }
        }

        tools
    }

    /// Call a tool on a specific MCP server
    pub async fn call_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        args: Value,
    ) -> Result<String, String> {
        self.ensure_connected(server_name).await?;

        let clients = self.clients.read().await;
        let server = clients
            .get(server_name)
            .ok_or_else(|| format!("Server '{}' not connected", server_name))?;

        debug!(server = server_name, tool = tool_name, "Calling MCP tool");

        // Build the call_tool request
        let arguments = match args {
            Value::Object(map) => Some(map),
            _ => None,
        };

        let tool_name_owned: std::borrow::Cow<'static, str> = tool_name.to_string().into();

        let result: CallToolResult = server
            .peer
            .call_tool(CallToolRequestParam {
                name: tool_name_owned,
                arguments,
            })
            .await
            .map_err(|e| format!("MCP tool call failed: {}", e))?;

        // Extract text content from the result
        let text: String = result
            .content
            .iter()
            .filter_map(|c| c.as_text().map(|t| t.text.to_string()))
            .collect::<Vec<_>>()
            .join("\n");

        if text.is_empty() {
            Ok("(empty result)".to_string())
        } else {
            Ok(text)
        }
    }

    /// Shutdown all connected MCP servers
    pub async fn shutdown(&self) {
        let mut clients = self.clients.write().await;
        let names: Vec<String> = clients.keys().cloned().collect();
        for name in names {
            if let Some(_server) = clients.remove(&name) {
                info!(server = %name, "Disconnecting from MCP server");
                // The peer is dropped here, which closes the connection.
                // The TokioChildProcess handles cleanup on drop.
            }
        }
    }

    /// Check if any MCP servers are configured
    pub fn has_servers(&self) -> bool {
        !self.configs.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_mcp_servers_basic() {
        let config: Value = serde_json::json!({
            "mcpServers": {
                "context7": {
                    "command": "npx",
                    "args": ["-y", "@context7/mcp"],
                    "env": {"API_KEY": "test"}
                },
                "mira": {
                    "command": "mira",
                    "args": ["serve"]
                }
            }
        });

        let mut configs = Vec::new();
        let mut seen = std::collections::HashSet::new();
        McpClientManager::parse_mcp_servers(&config, &mut configs, &mut seen);

        // "mira" should be filtered out
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].name, "context7");
        assert_eq!(configs[0].command, "npx");
        assert_eq!(configs[0].args, vec!["-y", "@context7/mcp"]);
        assert_eq!(configs[0].env.get("API_KEY").unwrap(), "test");
    }

    #[test]
    fn test_parse_mcp_servers_dedup() {
        let config: Value = serde_json::json!({
            "mcpServers": {
                "server1": {"command": "cmd1", "args": []},
                "server2": {"command": "cmd2", "args": []}
            }
        });

        let mut configs = Vec::new();
        let mut seen = std::collections::HashSet::new();

        // Parse once
        McpClientManager::parse_mcp_servers(&config, &mut configs, &mut seen);
        assert_eq!(configs.len(), 2);

        // Parse again - should not add duplicates
        McpClientManager::parse_mcp_servers(&config, &mut configs, &mut seen);
        assert_eq!(configs.len(), 2);
    }

    #[test]
    fn test_parse_mcp_servers_no_servers() {
        let config: Value = serde_json::json!({"other": "data"});
        let mut configs = Vec::new();
        let mut seen = std::collections::HashSet::new();
        McpClientManager::parse_mcp_servers(&config, &mut configs, &mut seen);
        assert!(configs.is_empty());
    }

    #[test]
    fn test_parse_mcp_servers_missing_command() {
        let config: Value = serde_json::json!({
            "mcpServers": {
                "no_cmd": {"args": ["arg1"]},
                "has_cmd": {"command": "test", "args": []}
            }
        });

        let mut configs = Vec::new();
        let mut seen = std::collections::HashSet::new();
        McpClientManager::parse_mcp_servers(&config, &mut configs, &mut seen);

        // Only server with command should be included
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].name, "has_cmd");
    }

    #[test]
    fn test_has_servers() {
        let manager = McpClientManager {
            configs: vec![],
            clients: Arc::new(RwLock::new(HashMap::new())),
        };
        assert!(!manager.has_servers());

        let manager = McpClientManager {
            configs: vec![McpServerConfig {
                name: "test".to_string(),
                command: "cmd".to_string(),
                args: vec![],
                env: HashMap::new(),
            }],
            clients: Arc::new(RwLock::new(HashMap::new())),
        };
        assert!(manager.has_servers());
    }
}
