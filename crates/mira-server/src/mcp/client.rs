// crates/mira-server/src/mcp_client.rs
// MCP Client Manager - connects to external MCP servers

use crate::tools::core::McpToolInfo;
use rmcp::model::{CallToolRequestParams, CallToolResult, ClientInfo};
use rmcp::service::{Peer, RunningService};
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::transport::child_process::TokioChildProcess;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::{RoleClient, serve_client};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use tokio::process::Command;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

// ---- Typed config structs for serde deserialization ----

/// Root of a .mcp.json file
#[derive(Deserialize)]
struct McpJsonRoot {
    #[serde(rename = "mcpServers", default)]
    mcp_servers: HashMap<String, ServerEntry>,
}

/// Root of a Codex config.toml file
#[derive(Deserialize)]
struct CodexTomlRoot {
    #[serde(default)]
    mcp_servers: HashMap<String, ServerEntry>,
}

/// A single server entry (works for both JSON and TOML formats).
/// Fields for both stdio and HTTP transports are optional; `command` takes
/// precedence over `url` when both are present.
#[derive(Deserialize)]
struct ServerEntry {
    command: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: HashMap<String, String>,
    cwd: Option<String>,
    url: Option<String>,
    bearer_token_env_var: Option<String>,
    #[serde(default)]
    http_headers: HashMap<String, String>,
    #[serde(default)]
    env_http_headers: HashMap<String, String>,
}

impl ServerEntry {
    fn into_transport(self) -> Option<McpTransport> {
        if let Some(command) = self.command {
            Some(McpTransport::Stdio {
                command,
                args: self.args,
                env: self.env,
                cwd: self.cwd,
            })
        } else if let Some(url) = self.url {
            Some(McpTransport::Http {
                url,
                bearer_token_env_var: self.bearer_token_env_var,
                http_headers: self.http_headers,
                env_http_headers: self.env_http_headers,
            })
        } else {
            None
        }
    }
}

/// Configuration for an external MCP server
#[derive(Debug, Clone)]
pub struct McpServerConfig {
    pub name: String,
    pub transport: McpTransport,
}

#[derive(Debug, Clone)]
pub enum McpTransport {
    Stdio {
        command: String,
        args: Vec<String>,
        env: HashMap<String, String>,
        cwd: Option<String>,
    },
    Http {
        url: String,
        bearer_token_env_var: Option<String>,
        http_headers: HashMap<String, String>,
        env_http_headers: HashMap<String, String>,
    },
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
    /// Per-server connection guards to prevent double-connect races.
    /// If a server name has a Notify, a connection attempt is in progress.
    /// Waiters await the Notify instead of polling.
    connecting: tokio::sync::Mutex<HashMap<String, Arc<tokio::sync::Notify>>>,
    /// Timeout for individual MCP tool calls
    mcp_tool_timeout: std::time::Duration,
}

impl McpClientManager {
    /// Create a new manager from MCP config files
    /// Reads .mcp.json and Codex config.toml from project path and global locations.
    pub fn from_mcp_configs(project_path: Option<&str>) -> Self {
        let mut configs = Vec::new();
        let mut seen_names = std::collections::HashSet::new();

        // Project-level configs take precedence
        if let Some(path) = project_path {
            let base = Path::new(path);
            Self::try_load_mcp_json(&base.join(".mcp.json"), &mut configs, &mut seen_names);
            Self::try_load_codex_toml(
                &base.join(".codex/config.toml"),
                &mut configs,
                &mut seen_names,
            );
        }

        // Global configs (lower precedence, deduped by seen_names)
        if let Some(home) = dirs::home_dir() {
            Self::try_load_mcp_json(
                &home.join(".claude/mcp.json"),
                &mut configs,
                &mut seen_names,
            );
            Self::try_load_codex_toml(
                &home.join(".codex/config.toml"),
                &mut configs,
                &mut seen_names,
            );
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
            connecting: tokio::sync::Mutex::new(HashMap::new()),
            mcp_tool_timeout: std::time::Duration::from_secs(60),
        }
    }

    /// Try to load MCP servers from a .mcp.json file.
    fn try_load_mcp_json(
        path: &Path,
        configs: &mut Vec<McpServerConfig>,
        seen: &mut std::collections::HashSet<String>,
    ) {
        if let Ok(content) = std::fs::read_to_string(path)
            && let Ok(root) = serde_json::from_str::<McpJsonRoot>(&content)
        {
            Self::add_servers(root.mcp_servers, configs, seen);
        }
    }

    /// Try to load MCP servers from a Codex config.toml file.
    fn try_load_codex_toml(
        path: &Path,
        configs: &mut Vec<McpServerConfig>,
        seen: &mut std::collections::HashSet<String>,
    ) {
        if let Ok(content) = std::fs::read_to_string(path)
            && let Ok(root) = toml::from_str::<CodexTomlRoot>(&content)
        {
            Self::add_servers(root.mcp_servers, configs, seen);
        }
    }

    /// Convert parsed server entries into configs, deduplicating by name.
    /// Shared by both JSON and TOML loaders.
    fn add_servers(
        servers: HashMap<String, ServerEntry>,
        configs: &mut Vec<McpServerConfig>,
        seen: &mut std::collections::HashSet<String>,
    ) {
        for (name, entry) in servers {
            if name == "mira" || seen.contains(&name) {
                continue;
            }
            if let Some(transport) = entry.into_transport() {
                seen.insert(name.clone());
                configs.push(McpServerConfig { name, transport });
            }
        }
    }

    /// Resolve the bearer token from an env var name, if configured.
    fn resolve_bearer_token(
        server_name: &str,
        bearer_token_env_var: &Option<String>,
    ) -> Option<String> {
        bearer_token_env_var
            .as_ref()
            .and_then(|env_var| match std::env::var(env_var) {
                Ok(token) => Some(token),
                Err(_) => {
                    warn!(
                        server = %server_name,
                        env_var = %env_var,
                        "Missing bearer token env var for MCP HTTP server"
                    );
                    None
                }
            })
    }

    /// Ensure a server is connected, lazily connecting if needed.
    /// Uses a per-server guard to prevent double-connect races.
    async fn ensure_connected(&self, server_name: &str) -> Result<(), String> {
        // Fast path: already connected
        {
            let clients = self.clients.read().await;
            if clients.contains_key(server_name) {
                return Ok(());
            }
        }

        // Acquire the connecting guard to prevent concurrent connection attempts
        {
            let mut connecting = self.connecting.lock().await;
            if let Some(notify) = connecting.get(server_name) {
                // Another task is already connecting — wait for notification
                let notify = notify.clone();
                drop(connecting);
                let timeout =
                    tokio::time::timeout(std::time::Duration::from_secs(10), notify.notified());
                if timeout.await.is_err() {
                    return Err(format!(
                        "Timed out waiting for concurrent connection to '{}'",
                        server_name
                    ));
                }
                // Check if it actually connected
                let clients = self.clients.read().await;
                if clients.contains_key(server_name) {
                    return Ok(());
                }
                return Err(format!("Concurrent connection to '{}' failed", server_name));
            }
            connecting.insert(
                server_name.to_string(),
                Arc::new(tokio::sync::Notify::new()),
            );
        }

        // Re-check after acquiring guard (another task may have completed between our check and guard)
        {
            let clients = self.clients.read().await;
            if clients.contains_key(server_name) {
                let mut connecting = self.connecting.lock().await;
                connecting.remove(server_name);
                return Ok(());
            }
        }

        // Actually connect — cleanup guard and notify waiters on both success and failure
        let result = self.do_connect(server_name).await;
        let mut connecting = self.connecting.lock().await;
        if let Some(notify) = connecting.remove(server_name) {
            notify.notify_waiters();
        }
        result
    }

    /// Perform the actual connection to an MCP server.
    async fn do_connect(&self, server_name: &str) -> Result<(), String> {
        // Find the config
        let config = self
            .configs
            .iter()
            .find(|c| c.name == server_name)
            .ok_or_else(|| format!("MCP server '{}' not configured", server_name))?
            .clone();

        // Connect as MCP client
        let client_info = ClientInfo {
            meta: None,
            protocol_version: Default::default(),
            capabilities: Default::default(),
            client_info: rmcp::model::Implementation {
                name: "mira".into(),
                title: Some("Mira MCP Client".into()),
                version: env!("CARGO_PKG_VERSION").into(),
                icons: None,
                website_url: None,
            },
        };
        let service = match &config.transport {
            McpTransport::Stdio {
                command,
                args,
                env,
                cwd,
            } => {
                // Security: log the full command being spawned so users can audit config behavior
                let env_keys: Vec<&str> = env.keys().map(|k| k.as_str()).collect();
                warn!(
                    server = %server_name,
                    command = %command,
                    args = ?args,
                    env_vars = ?env_keys,
                    cwd = ?cwd,
                    "Spawning MCP server child process from MCP config files"
                );

                // Build the command
                let mut cmd = Command::new(command);
                cmd.args(args);
                if let Some(cwd) = cwd {
                    cmd.current_dir(cwd);
                }
                for (key, value) in env {
                    cmd.env(key, value);
                }
                cmd.stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null()); // Suppress server stderr

                // Spawn the child process transport
                let transport = TokioChildProcess::new(cmd)
                    .map_err(|e| format!("Failed to spawn MCP server '{}': {}", server_name, e))?;

                serve_client(client_info, transport).await.map_err(|e| {
                    format!(
                        "Failed to initialize MCP client for '{}': {}",
                        server_name, e
                    )
                })?
            }
            McpTransport::Http {
                url,
                bearer_token_env_var,
                http_headers,
                env_http_headers,
            } => {
                info!(
                    server = %server_name,
                    url = %url,
                    "Connecting to MCP HTTP server"
                );

                // Warn about unsupported custom headers — rmcp's transport config
                // only supports bearer auth, not arbitrary headers.
                if !http_headers.is_empty() || !env_http_headers.is_empty() {
                    warn!(
                        server = %server_name,
                        "http_headers and env_http_headers are not supported for MCP HTTP transport; \
                         only bearer_token_env_var is used for authentication"
                    );
                }

                let auth_token = Self::resolve_bearer_token(server_name, bearer_token_env_var);

                let mut config = StreamableHttpClientTransportConfig::with_uri(url.as_str());
                if let Some(token) = auth_token {
                    config = config.auth_header(token);
                }

                let transport = StreamableHttpClientTransport::from_config(config);
                serve_client(client_info, transport).await.map_err(|e| {
                    format!(
                        "Failed to initialize MCP HTTP client for '{}': {}",
                        server_name, e
                    )
                })?
            }
        };

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

    /// Connect to all configured servers and call a closure for each connected server.
    /// Handles connection errors by logging and skipping.
    async fn for_each_connected_server<F, T>(&self, mut f: F) -> Vec<T>
    where
        F: FnMut(&str, &ConnectedServer) -> Vec<T>,
    {
        let mut results = Vec::new();

        for config in &self.configs {
            if let Err(e) = self.ensure_connected(&config.name).await {
                warn!(server = %config.name, error = %e, "Failed to connect to MCP server");
                continue;
            }

            let clients = self.clients.read().await;
            if let Some(server) = clients.get(&config.name) {
                results.extend(f(&config.name, server));
            }
        }

        results
    }

    /// List tools from all configured MCP servers
    /// Returns Vec of (server_name, tools) pairs
    pub async fn list_tools(&self) -> Vec<(String, Vec<McpToolInfo>)> {
        self.for_each_connected_server(|name, server| {
            let tools: Vec<McpToolInfo> = server
                .tools
                .iter()
                .map(|t| McpToolInfo {
                    name: t.name.to_string(),
                    description: t.description.as_deref().unwrap_or("").to_string(),
                })
                .collect();

            if tools.is_empty() {
                vec![]
            } else {
                vec![(name.to_string(), tools)]
            }
        })
        .await
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

        let result: CallToolResult = tokio::time::timeout(
            self.mcp_tool_timeout,
            server.peer.call_tool(CallToolRequestParams {
                meta: None,
                name: tool_name_owned,
                arguments,
                task: None,
            }),
        )
        .await
        .map_err(|_| {
            format!(
                "MCP tool call timed out after {}s",
                self.mcp_tool_timeout.as_secs()
            )
        })?
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

    /// Set the timeout for individual MCP tool calls
    pub fn set_mcp_tool_timeout(&mut self, timeout: std::time::Duration) {
        self.mcp_tool_timeout = timeout;
    }

    /// Check if any MCP servers are configured
    pub fn has_servers(&self) -> bool {
        !self.configs.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Helper: deserialize JSON and run add_servers, mirroring try_load_mcp_json.
    fn parse_json(
        json: &str,
        configs: &mut Vec<McpServerConfig>,
        seen: &mut std::collections::HashSet<String>,
    ) {
        let root: McpJsonRoot = serde_json::from_str(json).unwrap();
        McpClientManager::add_servers(root.mcp_servers, configs, seen);
    }

    #[test]
    fn test_parse_mcp_servers_basic() {
        let json = r#"{
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
        }"#;

        let mut configs = Vec::new();
        let mut seen = std::collections::HashSet::new();
        parse_json(json, &mut configs, &mut seen);

        // "mira" should be filtered out
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].name, "context7");
        match &configs[0].transport {
            McpTransport::Stdio {
                command, args, env, ..
            } => {
                assert_eq!(command, "npx");
                assert_eq!(args, &["-y", "@context7/mcp"]);
                assert_eq!(env.get("API_KEY").unwrap(), "test");
            }
            McpTransport::Http { .. } => panic!("Expected stdio transport"),
        }
    }

    #[test]
    fn test_parse_mcp_servers_dedup() {
        let json = r#"{
            "mcpServers": {
                "server1": {"command": "cmd1"},
                "server2": {"command": "cmd2"}
            }
        }"#;

        let mut configs = Vec::new();
        let mut seen = std::collections::HashSet::new();

        // Parse once
        parse_json(json, &mut configs, &mut seen);
        assert_eq!(configs.len(), 2);

        // Parse again - should not add duplicates
        parse_json(json, &mut configs, &mut seen);
        assert_eq!(configs.len(), 2);
    }

    #[test]
    fn test_parse_mcp_servers_no_servers() {
        let root: Result<McpJsonRoot, _> = serde_json::from_str(r#"{"other": "data"}"#);
        // mcpServers defaults to empty HashMap, so deserialization succeeds
        let root = root.unwrap();
        assert!(root.mcp_servers.is_empty());
    }

    #[test]
    fn test_parse_mcp_servers_missing_command() {
        let json = r#"{
            "mcpServers": {
                "no_cmd": {"args": ["arg1"]},
                "has_cmd": {"command": "test"}
            }
        }"#;

        let mut configs = Vec::new();
        let mut seen = std::collections::HashSet::new();
        parse_json(json, &mut configs, &mut seen);

        // Only server with command should be included
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].name, "has_cmd");
    }

    #[test]
    fn test_parse_http_server() {
        let json = r#"{
            "mcpServers": {
                "remote": {
                    "url": "https://example.com/mcp",
                    "bearer_token_env_var": "MY_TOKEN"
                }
            }
        }"#;

        let mut configs = Vec::new();
        let mut seen = std::collections::HashSet::new();
        parse_json(json, &mut configs, &mut seen);

        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].name, "remote");
        match &configs[0].transport {
            McpTransport::Http {
                url,
                bearer_token_env_var,
                ..
            } => {
                assert_eq!(url, "https://example.com/mcp");
                assert_eq!(bearer_token_env_var.as_deref(), Some("MY_TOKEN"));
            }
            McpTransport::Stdio { .. } => panic!("Expected HTTP transport"),
        }
    }

    #[test]
    fn test_parse_codex_toml() {
        let toml_str = r#"
            [mcp_servers.myserver]
            command = "my-mcp"
            args = ["--port", "8080"]
        "#;

        let root: CodexTomlRoot = toml::from_str(toml_str).unwrap();
        let mut configs = Vec::new();
        let mut seen = std::collections::HashSet::new();
        McpClientManager::add_servers(root.mcp_servers, &mut configs, &mut seen);

        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].name, "myserver");
        match &configs[0].transport {
            McpTransport::Stdio { command, args, .. } => {
                assert_eq!(command, "my-mcp");
                assert_eq!(args, &["--port", "8080"]);
            }
            McpTransport::Http { .. } => panic!("Expected stdio transport"),
        }
    }

    // ========================================================================
    // $schema stripping (provider compatibility)
    // ========================================================================

    /// Helper that mirrors the inline $schema stripping logic from get_all_tools
    fn strip_schema_field(mut value: Value) -> Value {
        if let Some(obj) = value.as_object_mut() {
            obj.remove("$schema");
        }
        value
    }

    #[test]
    fn test_strip_schema_removes_schema_field() {
        let input = json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "properties": {"name": {"type": "string"}}
        });
        let result = strip_schema_field(input);
        assert!(result.get("$schema").is_none());
        assert_eq!(result.get("type").unwrap(), "object");
        assert!(result.get("properties").is_some());
    }

    #[test]
    fn test_strip_schema_no_schema_field_unchanged() {
        let input = json!({
            "type": "object",
            "properties": {"id": {"type": "integer"}}
        });
        let result = strip_schema_field(input.clone());
        assert_eq!(result, input);
    }

    #[test]
    fn test_strip_schema_non_object_unchanged() {
        let input = json!("just a string");
        let result = strip_schema_field(input.clone());
        assert_eq!(result, input);
    }

    #[test]
    fn test_has_servers() {
        let manager = McpClientManager {
            configs: vec![],
            clients: Arc::new(RwLock::new(HashMap::new())),
            connecting: tokio::sync::Mutex::new(HashMap::new()),
            mcp_tool_timeout: std::time::Duration::from_secs(60),
        };
        assert!(!manager.has_servers());

        let manager = McpClientManager {
            configs: vec![McpServerConfig {
                name: "test".to_string(),
                transport: McpTransport::Stdio {
                    command: "cmd".to_string(),
                    args: vec![],
                    env: HashMap::new(),
                    cwd: None,
                },
            }],
            clients: Arc::new(RwLock::new(HashMap::new())),
            connecting: tokio::sync::Mutex::new(HashMap::new()),
            mcp_tool_timeout: std::time::Duration::from_secs(60),
        };
        assert!(manager.has_servers());
    }
}
