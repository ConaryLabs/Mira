//! Connect - Stdio shim that bridges Claude Code to the Mira daemon
//!
//! This provides a thin protocol bridge:
//! - Reads MCP JSON-RPC messages from stdin
//! - Forwards them to the daemon via HTTP
//! - Writes responses to stdout
//! - Maintains session state via mcp-session-id header
//!
//! Claude Code config:
//! ```json
//! {
//!   "mcp": {
//!     "command": "/path/to/mira",
//!     "args": ["connect"]
//!   }
//! }
//! ```

use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::Value;
use std::io::{self, BufRead, Write};
use std::time::Duration;

const TOKEN_FILE: &str = ".mira/token";

/// Load auth token from ~/.mira/token
fn load_token() -> Result<String> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("No home directory"))?;
    let token_path = home.join(TOKEN_FILE);

    if token_path.exists() {
        let token = std::fs::read_to_string(&token_path)?;
        Ok(token.trim().to_string())
    } else {
        anyhow::bail!(
            "Auth token not found at {}. Start the daemon first with: mira",
            token_path.display()
        );
    }
}

/// Check if daemon is running
async fn check_daemon(client: &Client, daemon_url: &str) -> Result<()> {
    let health_url = format!("{}/health", daemon_url);

    client
        .get(&health_url)
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .context("Cannot connect to Mira daemon")?
        .error_for_status()
        .context("Daemon health check failed")?;

    Ok(())
}

/// Run the stdio shim
pub async fn run(daemon_url: String) -> Result<()> {
    // Load auth token
    let token = load_token()?;

    // Create HTTP client
    let client = Client::builder()
        .timeout(Duration::from_secs(300)) // Long timeout for streaming
        .build()?;

    // Check daemon is running
    check_daemon(&client, &daemon_url).await.map_err(|e| {
        eprintln!("Error: {}", e);
        eprintln!("Is the Mira daemon running? Start with: mira");
        e
    })?;

    let mcp_url = format!("{}/mcp", daemon_url);

    // Track session ID from initialize response
    let mut session_id: Option<String> = None;

    // Read stdin line by line (MCP uses newline-delimited JSON)
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = line.context("Failed to read from stdin")?;

        if line.trim().is_empty() {
            continue;
        }

        // Parse as JSON to validate
        let request: Value = serde_json::from_str(&line)
            .context("Invalid JSON from stdin")?;

        // Build request with auth
        let mut req = client
            .post(&mcp_url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream");

        // Include session ID if we have one (required after initialize)
        if let Some(ref sid) = session_id {
            req = req.header("mcp-session-id", sid);
        }

        // Forward to daemon
        let response = req
            .json(&request)
            .send()
            .await
            .context("Failed to forward request to daemon")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            eprintln!("Daemon error ({}): {}", status, body);
            continue;
        }

        // Capture session ID from response headers (set after initialize)
        if let Some(sid) = response.headers().get("mcp-session-id") {
            if let Ok(sid_str) = sid.to_str() {
                session_id = Some(sid_str.to_string());
            }
        }

        // Get response body and parse SSE format
        let response_text = response.text().await
            .context("Failed to read daemon response")?;

        // Parse SSE format - extract JSON from "data: {...}" lines
        for line in response_text.lines() {
            if let Some(json_data) = line.strip_prefix("data: ") {
                writeln!(stdout, "{}", json_data)
                    .context("Failed to write to stdout")?;
            }
        }
        stdout.flush()?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_path() {
        // Just verify the path construction works
        if let Some(home) = dirs::home_dir() {
            let path = home.join(TOKEN_FILE);
            assert!(path.to_string_lossy().contains(".mira/token"));
        }
    }
}
