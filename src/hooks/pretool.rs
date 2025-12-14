//! PreToolUse hook for Claude Code
//!
//! Fires before Edit/Read/Write operations to surface related code context.
//! Helps the LLM understand impact of changes before making them.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::io::Read;

#[derive(Debug, Deserialize)]
struct HookInput {
    hook_event_name: String,
    tool_name: Option<String>,
    tool_input: Option<serde_json::Value>,
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

// Debounce - don't spam context for the same file
const DEBOUNCE_FILE: &str = "/tmp/mira-pretool-debounce.json";
const DEBOUNCE_SECONDS: u64 = 120; // 2 minutes between context for same file

pub async fn run() -> Result<()> {
    // Read stdin
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;

    // Parse hook input
    let hook_input: HookInput = match serde_json::from_str(&input) {
        Ok(h) => h,
        Err(_) => return Ok(()), // Invalid JSON, exit silently
    };

    // Only process PreToolUse events
    if hook_input.hook_event_name != "PreToolUse" {
        return Ok(());
    }

    let tool_name = match hook_input.tool_name {
        Some(t) => t,
        None => return Ok(()),
    };

    // Only provide context for file operations
    if !matches!(tool_name.as_str(), "Edit" | "Write") {
        return Ok(());
    }

    let tool_input = hook_input.tool_input.unwrap_or(serde_json::json!({}));

    // Get file path from tool input
    let file_path = match tool_input.get("file_path").and_then(|p| p.as_str()) {
        Some(p) => p.to_string(),
        None => return Ok(()),
    };

    // Skip non-code files
    if !is_code_file(&file_path) {
        return Ok(());
    }

    // Check debounce
    if !should_provide_context(&file_path) {
        return Ok(());
    }

    // Get code context from Mira
    let context = match get_code_context(&file_path).await {
        Some(c) => c,
        None => return Ok(()),
    };

    // Only output if we have meaningful context
    if context.is_empty() {
        return Ok(());
    }

    // Mark as provided
    mark_context_provided(&file_path);

    let output = HookOutput {
        hook_specific_output: HookSpecificOutput {
            hook_event_name: "PreToolUse".to_string(),
            system_message: Some(context),
        },
    };
    println!("{}", serde_json::to_string(&output)?);

    Ok(())
}

fn is_code_file(path: &str) -> bool {
    let code_extensions = [
        ".rs", ".py", ".js", ".ts", ".tsx", ".jsx", ".go", ".java",
        ".c", ".cpp", ".h", ".hpp", ".rb", ".php", ".swift", ".kt",
    ];
    code_extensions.iter().any(|ext| path.ends_with(ext))
}

fn should_provide_context(file_path: &str) -> bool {
    let debounce = load_debounce();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    if let Some(last_time) = debounce.get(file_path).and_then(|v| v.as_u64()) {
        if now - last_time < DEBOUNCE_SECONDS {
            return false;
        }
    }
    true
}

fn mark_context_provided(file_path: &str) {
    let mut debounce = load_debounce();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    debounce[file_path] = serde_json::json!(now);

    // Clean old entries
    let cutoff = now.saturating_sub(3600);
    let keys_to_remove: Vec<String> = debounce
        .as_object()
        .map(|obj| {
            obj.iter()
                .filter(|(_, v)| v.as_u64().unwrap_or(0) < cutoff)
                .map(|(k, _)| k.clone())
                .collect()
        })
        .unwrap_or_default();

    for key in keys_to_remove {
        debounce.as_object_mut().map(|obj| obj.remove(&key));
    }

    if let Ok(json) = serde_json::to_string(&debounce) {
        let _ = std::fs::write(DEBOUNCE_FILE, json);
    }
}

fn load_debounce() -> serde_json::Value {
    std::fs::read_to_string(DEBOUNCE_FILE)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::json!({}))
}

async fn get_code_context(file_path: &str) -> Option<String> {
    let mira_url = std::env::var("MIRA_URL")
        .unwrap_or_else(|_| "http://localhost:3000/mcp".to_string());
    let auth_token = std::env::var("MIRA_AUTH_TOKEN")
        .unwrap_or_else(|_| "63c7663fe0dbdfcd2bbf6c33a0a9b4b9".to_string());

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .ok()?;

    // Initialize MCP session
    let init_req = McpRequest {
        jsonrpc: "2.0".to_string(),
        id: 1,
        method: "initialize".to_string(),
        params: serde_json::json!({
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "pretool-hook", "version": "1.0"}
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

    // Get related files
    let related_req = McpRequest {
        jsonrpc: "2.0".to_string(),
        id: 2,
        method: "tools/call".to_string(),
        params: serde_json::json!({
            "name": "get_related_files",
            "arguments": {
                "file_path": file_path,
                "relation_type": "all",
                "limit": 5
            }
        }),
    };

    let resp = client
        .post(&mira_url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .header("Authorization", format!("Bearer {}", auth_token))
        .header("Mcp-Session-Id", &mcp_session)
        .json(&related_req)
        .send()
        .await
        .ok()?;

    let body = resp.text().await.ok()?;

    // Parse the SSE response to extract useful info
    let mut context_parts = Vec::new();

    // Look for cochange patterns in response
    if body.contains("cochange") {
        // Extract file names from cochange patterns
        let mut cochange_files = Vec::new();
        for line in body.lines() {
            if line.contains("\"file\"") {
                // Simple extraction - find quoted file paths
                if let Some(start) = line.find("\"file\":") {
                    let rest = &line[start + 8..];
                    if let Some(end) = rest.find('"') {
                        let file_val = &rest[..end];
                        if let Some(start2) = file_val.find('"') {
                            let file = &file_val[start2 + 1..];
                            if !file.is_empty() && !file.contains('{') {
                                cochange_files.push(file.to_string());
                            }
                        }
                    }
                }
            }
        }

        if !cochange_files.is_empty() {
            let files_display: Vec<&str> = cochange_files.iter().take(3).map(|s| s.as_str()).collect();
            context_parts.push(format!(
                "Files that often change together: {}",
                files_display.join(", ")
            ));
        }
    }

    // Get key symbols from the file
    let symbols_req = McpRequest {
        jsonrpc: "2.0".to_string(),
        id: 3,
        method: "tools/call".to_string(),
        params: serde_json::json!({
            "name": "get_symbols",
            "arguments": {
                "file_path": file_path
            }
        }),
    };

    let resp = client
        .post(&mira_url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .header("Authorization", format!("Bearer {}", auth_token))
        .header("Mcp-Session-Id", &mcp_session)
        .json(&symbols_req)
        .send()
        .await
        .ok()?;

    let symbols_body = resp.text().await.ok()?;

    // Count symbols
    let function_count = symbols_body.matches("\"type\":\"function\"").count();
    let struct_count = symbols_body.matches("\"type\":\"struct\"").count()
        + symbols_body.matches("\"type\":\"class\"").count();

    if function_count > 0 || struct_count > 0 {
        let mut parts = Vec::new();
        if function_count > 0 {
            parts.push(format!("{} functions", function_count));
        }
        if struct_count > 0 {
            parts.push(format!("{} types", struct_count));
        }
        context_parts.push(format!("File contains: {}", parts.join(", ")));
    }

    // Get improvement suggestions via proactive context
    let proactive_req = McpRequest {
        jsonrpc: "2.0".to_string(),
        id: 4,
        method: "tools/call".to_string(),
        params: serde_json::json!({
            "name": "get_proactive_context",
            "arguments": {
                "files": [file_path],
                "limit_per_category": 3
            }
        }),
    };

    let improvements_resp = client
        .post(&mira_url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .header("Authorization", format!("Bearer {}", auth_token))
        .header("Mcp-Session-Id", &mcp_session)
        .json(&proactive_req)
        .send()
        .await
        .ok();

    if let Some(resp) = improvements_resp {
        if let Ok(improvements_text) = resp.text().await {
            // Parse improvements from response
            if let Some(improvements) = extract_improvements(&improvements_text) {
                if !improvements.is_empty() {
                    context_parts.push(improvements);
                }
            }
        }
    }

    if context_parts.is_empty() {
        return None;
    }

    // Format filename nicely
    let filename = std::path::Path::new(file_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(file_path);

    Some(format!(
        "Code context for {}: {}",
        filename,
        context_parts.join(". ")
    ))
}

/// Extract improvement suggestions from proactive context response
fn extract_improvements(response: &str) -> Option<String> {
    // The response is SSE format - find JSON data events
    for line in response.lines() {
        if let Some(json_start) = line.find("improvement_suggestions") {
            // Found improvements - try to parse the array
            if let Some(arr_start) = line[json_start..].find('[') {
                let rest = &line[json_start + arr_start..];
                // Find matching bracket
                let mut depth = 0;
                let mut end_pos = 0;
                for (i, c) in rest.chars().enumerate() {
                    match c {
                        '[' => depth += 1,
                        ']' => {
                            depth -= 1;
                            if depth == 0 {
                                end_pos = i + 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                }

                if end_pos > 0 {
                    let arr_json = &rest[..end_pos];
                    if let Ok(improvements) = serde_json::from_str::<Vec<serde_json::Value>>(arr_json) {
                        if improvements.is_empty() {
                            return None;
                        }

                        // Filter for high severity only in hook context
                        let high: Vec<_> = improvements.iter()
                            .filter(|i| i.get("severity").and_then(|s| s.as_str()) == Some("high"))
                            .collect();

                        if high.is_empty() {
                            return None;
                        }

                        let mut out = String::from("Code improvements needed:");
                        for imp in high.iter().take(3) {
                            let symbol = imp.get("symbol_name").and_then(|s| s.as_str()).unwrap_or("?");
                            let imp_type = imp.get("improvement_type").and_then(|s| s.as_str()).unwrap_or("?");
                            let current = imp.get("current_value").and_then(|v| v.as_i64()).unwrap_or(0);
                            let threshold = imp.get("threshold").and_then(|v| v.as_i64()).unwrap_or(0);
                            out.push_str(&format!(
                                " [{}] {} - {} lines (max: {})",
                                imp_type.replace('_', " "),
                                symbol,
                                current,
                                threshold
                            ));
                        }
                        return Some(out);
                    }
                }
            }
        }
    }
    None
}
