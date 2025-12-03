// backend/src/mcp/transport.rs
// Transport layer for MCP communication (stdio and HTTP)

use anyhow::{Context, Result};
use async_trait::async_trait;
use std::collections::HashMap;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tracing::{debug, warn};

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

/// HTTP transport for remote MCP servers (placeholder for future implementation)
#[allow(dead_code)]
pub struct HttpTransport {
    url: String,
    client: reqwest::Client,
}

#[allow(dead_code)]
impl HttpTransport {
    pub fn new(url: &str) -> Self {
        Self {
            url: url.to_string(),
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl McpTransport for HttpTransport {
    async fn send(&self, message: &str) -> Result<String> {
        let response = self
            .client
            .post(&self.url)
            .header("Content-Type", "application/json")
            .body(message.to_string())
            .send()
            .await
            .context("HTTP request failed")?;

        if !response.status().is_success() {
            anyhow::bail!("HTTP error: {}", response.status());
        }

        response.text().await.context("Failed to read response body")
    }

    fn is_connected(&self) -> bool {
        true // HTTP is connectionless, always "connected"
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
