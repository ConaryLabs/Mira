// backend/src/mcp/transport.rs
// Transport layer for MCP communication (stdio and HTTP)

use anyhow::{Context, Result};
use async_trait::async_trait;
use std::collections::HashMap;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tracing::debug;

/// Transport trait for MCP communication
#[async_trait]
pub trait McpTransport {
    /// Send a message and receive a response
    async fn send(&self, message: &str) -> Result<String>;

    /// Check if transport is connected
    fn is_connected(&self) -> bool;
}

/// Stdio transport for spawned MCP server processes
pub struct StdioTransport {
    #[allow(dead_code)]
    child: Mutex<Child>,
    stdin: Mutex<tokio::process::ChildStdin>,
    stdout: Mutex<BufReader<tokio::process::ChildStdout>>,
}

impl StdioTransport {
    /// Spawn a new MCP server process
    pub async fn spawn(
        command: &str,
        args: &[String],
        env: &HashMap<String, String>,
    ) -> Result<Self> {
        let mut cmd = Command::new(command);
        cmd.args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        // Add environment variables
        for (key, value) in env {
            cmd.env(key, value);
        }

        let mut child = cmd.spawn().context("Failed to spawn MCP server process")?;

        let stdin = child.stdin.take().context("Failed to get stdin")?;
        let stdout = child.stdout.take().context("Failed to get stdout")?;

        // Spawn a task to handle stderr logging
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(async move {
                let mut reader = BufReader::new(stderr);
                let mut line = String::new();
                while let Ok(n) = reader.read_line(&mut line).await {
                    if n == 0 {
                        break;
                    }
                    debug!("[MCP:stderr] {}", line.trim());
                    line.clear();
                }
            });
        }

        Ok(Self {
            child: Mutex::new(child),
            stdin: Mutex::new(stdin),
            stdout: Mutex::new(BufReader::new(stdout)),
        })
    }
}

#[async_trait]
impl McpTransport for StdioTransport {
    async fn send(&self, message: &str) -> Result<String> {
        // MCP uses newline-delimited JSON
        let mut stdin = self.stdin.lock().await;
        let mut stdout = self.stdout.lock().await;

        // Write message with newline
        stdin
            .write_all(message.as_bytes())
            .await
            .context("Failed to write to MCP stdin")?;
        stdin
            .write_all(b"\n")
            .await
            .context("Failed to write newline")?;
        stdin.flush().await.context("Failed to flush stdin")?;

        // Read response line
        let mut response = String::new();
        stdout
            .read_line(&mut response)
            .await
            .context("Failed to read from MCP stdout")?;

        if response.is_empty() {
            anyhow::bail!("MCP server closed connection");
        }

        Ok(response.trim().to_string())
    }

    fn is_connected(&self) -> bool {
        // Check if child process is still running
        // Note: We can't easily check this without try_wait which consumes the result
        true
    }
}

impl Drop for StdioTransport {
    fn drop(&mut self) {
        // Child process will be killed due to kill_on_drop(true)
        debug!("[MCP] Dropping stdio transport, killing child process");
    }
}

/// HTTP transport for remote MCP servers
/// Supports MCP's HTTP+SSE transport specification
pub struct HttpTransport {
    url: String,
    client: reqwest::Client,
    session_id: tokio::sync::RwLock<Option<String>>,
    timeout_ms: u64,
}

impl HttpTransport {
    /// Create a new HTTP transport with default timeout
    pub fn new(url: &str) -> Self {
        Self::with_timeout(url, 30_000)
    }

    /// Create with custom timeout in milliseconds
    pub fn with_timeout(url: &str, timeout_ms: u64) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(timeout_ms))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            url: url.to_string(),
            client,
            session_id: tokio::sync::RwLock::new(None),
            timeout_ms,
        }
    }

    /// Get the current session ID
    pub async fn session_id(&self) -> Option<String> {
        self.session_id.read().await.clone()
    }

    /// Set the session ID (extracted from server response)
    pub async fn set_session_id(&self, id: String) {
        *self.session_id.write().await = Some(id);
    }

    /// Clear the session ID (for reconnection)
    pub async fn clear_session(&self) {
        *self.session_id.write().await = None;
    }

    /// Send a ping to check connection
    pub async fn ping(&self) -> bool {
        let ping = r#"{"jsonrpc":"2.0","id":0,"method":"ping"}"#;
        self.send(ping).await.is_ok()
    }
}

#[async_trait]
impl McpTransport for HttpTransport {
    async fn send(&self, message: &str) -> Result<String> {
        let mut request = self
            .client
            .post(&self.url)
            .header("Content-Type", "application/json");

        // Add session ID if we have one
        if let Some(session) = self.session_id.read().await.as_ref() {
            request = request.header("X-MCP-Session-Id", session);
        }

        let response = request
            .body(message.to_string())
            .send()
            .await
            .context("HTTP request failed")?;

        // Extract session ID from response headers if present
        if let Some(session) = response.headers().get("X-MCP-Session-Id") {
            if let Ok(session_str) = session.to_str() {
                *self.session_id.write().await = Some(session_str.to_string());
            }
        }

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("HTTP error {}: {}", status, body);
        }

        response.text().await.context("Failed to read response body")
    }

    fn is_connected(&self) -> bool {
        // HTTP is connectionless, always "connected"
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_spawn_echo_server() {
        // Test with a simple echo command
        let result = StdioTransport::spawn(
            "cat",
            &[],
            &HashMap::new(),
        )
        .await;

        // cat should work on Unix systems
        if result.is_ok() {
            let transport = result.unwrap();
            let response = transport.send(r#"{"test": true}"#).await;
            assert!(response.is_ok());
            assert_eq!(response.unwrap(), r#"{"test": true}"#);
        }
    }

    #[test]
    fn test_http_transport_creation() {
        let transport = HttpTransport::new("http://localhost:3000/mcp");
        assert!(transport.is_connected());
    }
}
