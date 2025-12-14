//! SessionStart hook for Claude Code
//!
//! Fires at the start of each session to check for active work state
//! and prompt the user to resume if there's unfinished work.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::io::{self, Read};

#[derive(Debug, Deserialize)]
struct HookInput {
    hook_event_name: String,
    cwd: Option<String>,
}

#[derive(Debug, Serialize)]
struct HookOutput {
    #[serde(rename = "hookSpecificOutput")]
    hook_specific_output: HookSpecificOutput,
}

#[derive(Debug, Serialize)]
struct HookSpecificOutput {
    #[serde(rename = "hookEventName")]
    hook_event_name: String,
    #[serde(rename = "systemMessage", skip_serializing_if = "Option::is_none")]
    system_message: Option<String>,
}

// MCP types for HTTP API
#[derive(Debug, Serialize)]
struct McpRequest {
    jsonrpc: String,
    id: u32,
    method: String,
    params: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct McpNotification {
    jsonrpc: String,
    method: String,
}

pub async fn run() -> Result<()> {
    // Read stdin
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;

    // Parse hook input
    let hook_input: HookInput = match serde_json::from_str(&input) {
        Ok(h) => h,
        Err(_) => return Ok(()), // Invalid JSON, exit silently
    };

    // Only process SessionStart events
    if hook_input.hook_event_name != "SessionStart" {
        return Ok(());
    }

    // Get project path from cwd
    let project_path = hook_input.cwd.unwrap_or_else(|| ".".to_string());

    // Check for active work state
    let work_state = check_work_state(&project_path).await;

    // Only output if there's active work to resume
    if let Some(message) = work_state {
        let output = HookOutput {
            hook_specific_output: HookSpecificOutput {
                hook_event_name: "SessionStart".to_string(),
                system_message: Some(message),
            },
        };
        println!("{}", serde_json::to_string(&output)?);
    }

    Ok(())
}

async fn check_work_state(project_path: &str) -> Option<String> {
    let mira_url = std::env::var("MIRA_URL")
        .unwrap_or_else(|_| "http://localhost:3000/mcp".to_string());
    let auth_token = std::env::var("MIRA_AUTH_TOKEN")
        .unwrap_or_else(|_| "63c7663fe0dbdfcd2bbf6c33a0a9b4b9".to_string());

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .ok()?;

    // Initialize MCP session
    let init_req = McpRequest {
        jsonrpc: "2.0".to_string(),
        id: 1,
        method: "initialize".to_string(),
        params: serde_json::json!({
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": {"name": "sessionstart-hook", "version": "1.0"}
        }),
    };

    let resp = client
        .post(&mira_url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .header("Authorization", format!("Bearer {}", auth_token))
        .json(&init_req)
        .send()
        .await
        .ok()?;

    let mcp_session = resp
        .headers()
        .get("mcp-session-id")
        .and_then(|h| h.to_str().ok())?
        .to_string();

    // Send initialized notification
    let notif = McpNotification {
        jsonrpc: "2.0".to_string(),
        method: "notifications/initialized".to_string(),
    };

    client
        .post(&mira_url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .header("Authorization", format!("Bearer {}", auth_token))
        .header("Mcp-Session-Id", &mcp_session)
        .json(&notif)
        .send()
        .await
        .ok()?;

    // Call get_work_state
    let state_req = McpRequest {
        jsonrpc: "2.0".to_string(),
        id: 2,
        method: "tools/call".to_string(),
        params: serde_json::json!({
            "name": "get_work_state",
            "arguments": {}
        }),
    };

    let resp = client
        .post(&mira_url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .header("Authorization", format!("Bearer {}", auth_token))
        .header("Mcp-Session-Id", &mcp_session)
        .json(&state_req)
        .send()
        .await
        .ok()?;

    let body = resp.text().await.ok()?;

    // Parse the SSE response to extract work state
    let has_todos = body.contains("active_todos");
    let has_plan = body.contains("active_plan");
    let has_docs = body.contains("working_doc");

    if !has_todos && !has_plan && !has_docs {
        return None;
    }

    // Build prompt message
    let mut parts = vec![
        "PREVIOUS SESSION DETECTED: There is unfinished work from a previous session.".to_string(),
    ];

    if has_plan {
        parts.push("- An active PLAN was in progress".to_string());
    }
    if has_todos {
        parts.push("- Active TODOs were being tracked".to_string());
    }
    if has_docs {
        parts.push("- Working documents were created".to_string());
    }

    parts.push(format!("\nTo resume, run: session_start(project_path=\"{}\")", project_path));
    parts.push("To start fresh, just proceed with your request.".to_string());

    Some(parts.join("\n"))
}
