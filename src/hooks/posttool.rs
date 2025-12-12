//! PostToolCall hook for Claude Code
//!
//! Fires after each tool call to auto-remember significant actions.
//! Builds context passively so sessions have continuity even without explicit saves.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Deserialize)]
struct HookInput {
    hook_event_name: String,
    session_id: Option<String>,
    tool_name: Option<String>,
    tool_input: Option<serde_json::Value>,
    #[allow(dead_code)] // May use for richer context later
    tool_result: Option<serde_json::Value>,
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

// Debounce file to prevent duplicate saves
const DEBOUNCE_FILE: &str = "/tmp/mira-posttool-debounce.json";
const DEBOUNCE_SECONDS: u64 = 60;

pub async fn run() -> Result<()> {
    // Read stdin
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;

    // Parse hook input
    let hook_input: HookInput = match serde_json::from_str(&input) {
        Ok(h) => h,
        Err(_) => return Ok(()), // Invalid JSON, exit silently
    };

    // Only process PostToolUse events
    if hook_input.hook_event_name != "PostToolUse" {
        return Ok(());
    }

    let tool_name = match hook_input.tool_name {
        Some(t) => t,
        None => return Ok(()),
    };

    // Skip Mira tools to avoid recursion
    if tool_name.starts_with("mcp__mira__") {
        return Ok(());
    }

    let tool_input = hook_input.tool_input.unwrap_or(serde_json::json!({}));
    let session_id = hook_input.session_id.unwrap_or_else(|| "unknown".to_string());

    // Process based on tool type
    let action = match tool_name.as_str() {
        "Edit" | "Write" => extract_file_action(&tool_name, &tool_input),
        "Bash" => extract_bash_action(&tool_input),
        "Task" => extract_task_action(&tool_input),
        "Grep" => extract_grep_action(&tool_input),
        "WebSearch" => extract_search_action(&tool_input),
        _ => None,
    };

    // If we have a significant action, save it
    if let Some(action) = action {
        // Check debounce
        if !should_save(&action.key) {
            return Ok(());
        }

        // Save to Mira
        if let Err(e) = save_action(&session_id, &action).await {
            eprintln!("PostToolCall hook error: {}", e);
        }

        // Update debounce
        mark_saved(&action.key);
    }

    Ok(())
}

#[derive(Debug)]
struct Action {
    key: String,
    content: String,
    fact_type: String,
    category: String,
}

fn extract_file_action(tool_name: &str, input: &serde_json::Value) -> Option<Action> {
    let file_path = input.get("file_path").and_then(|p| p.as_str())?;

    // Skip temp files, node_modules, etc.
    if file_path.contains("/tmp/")
        || file_path.contains("node_modules")
        || file_path.contains(".git/")
        || file_path.contains("target/")
    {
        return None;
    }

    // Make path relative for readability
    let display_path = if file_path.contains("/Mira/") {
        file_path.split("/Mira/").last().unwrap_or(file_path)
    } else {
        file_path.split('/').last().unwrap_or(file_path)
    };

    let action_verb = if tool_name == "Edit" { "Edited" } else { "Created" };

    Some(Action {
        key: format!("file-{}", file_path),
        content: format!("{} file: {}", action_verb, display_path),
        fact_type: "context".to_string(),
        category: "session_activity".to_string(),
    })
}

fn extract_bash_action(input: &serde_json::Value) -> Option<Action> {
    let command = input.get("command").and_then(|c| c.as_str())?;

    // Only track significant commands
    let significant_patterns = [
        // Git operations
        ("git commit", "Made git commit"),
        ("git push", "Pushed to remote"),
        ("git pull", "Pulled from remote"),
        ("git checkout", "Switched branch"),
        ("git merge", "Merged branch"),
        ("git rebase", "Rebased branch"),
        // Rust
        ("cargo build", "Built Rust project"),
        ("cargo test", "Ran Rust tests"),
        ("cargo add", "Added Rust dependency"),
        ("cargo clippy", "Ran Rust linter"),
        // Node/JS
        ("npm install", "Installed npm packages"),
        ("npm run build", "Built npm project"),
        ("npm run test", "Ran npm tests"),
        ("yarn add", "Added yarn package"),
        // Python
        ("pip install", "Installed Python package"),
        ("pytest", "Ran Python tests"),
        ("python -m", "Ran Python module"),
        // Docker
        ("docker build", "Built Docker image"),
        ("docker-compose up", "Started Docker services"),
        ("docker run", "Ran Docker container"),
        // System
        ("systemctl", "Modified system service"),
        ("make", "Ran make"),
    ];

    for (pattern, description) in &significant_patterns {
        if command.contains(pattern) {
            // Extract more context
            let detail = if command.len() > 50 {
                format!("{}...", &command[..50])
            } else {
                command.to_string()
            };

            return Some(Action {
                key: format!("cmd-{}-{}", pattern, timestamp_minute()),
                content: format!("{}: {}", description, detail),
                fact_type: "context".to_string(),
                category: "session_activity".to_string(),
            });
        }
    }

    None
}

fn extract_task_action(input: &serde_json::Value) -> Option<Action> {
    // Track when agents are spawned for significant work
    let prompt = input.get("prompt").and_then(|p| p.as_str())?;
    let subagent_type = input.get("subagent_type").and_then(|s| s.as_str()).unwrap_or("unknown");

    // Only track if the prompt is substantial
    if prompt.len() < 50 {
        return None;
    }

    let summary = if prompt.len() > 100 {
        format!("{}...", &prompt[..100])
    } else {
        prompt.to_string()
    };

    Some(Action {
        key: format!("task-{}-{}", subagent_type, timestamp_minute()),
        content: format!("Spawned {} agent: {}", subagent_type, summary),
        fact_type: "context".to_string(),
        category: "session_activity".to_string(),
    })
}

fn extract_grep_action(input: &serde_json::Value) -> Option<Action> {
    let pattern = input.get("pattern").and_then(|p| p.as_str())?;

    // Only track meaningful search patterns (not single chars or very short)
    if pattern.len() < 4 {
        return None;
    }

    // Skip common/noisy patterns
    let skip_patterns = ["TODO", "FIXME", "import", "use ", "from "];
    if skip_patterns.iter().any(|s| pattern.contains(s)) {
        return None;
    }

    let display = if pattern.len() > 40 {
        format!("{}...", &pattern[..37])
    } else {
        pattern.to_string()
    };

    Some(Action {
        key: format!("grep-{}", timestamp_minute()),
        content: format!("Searched for: {}", display),
        fact_type: "context".to_string(),
        category: "research".to_string(),
    })
}

fn extract_search_action(input: &serde_json::Value) -> Option<Action> {
    let query = input.get("query").and_then(|q| q.as_str())?;

    // Track web searches - useful for understanding research context
    let display = if query.len() > 60 {
        format!("{}...", &query[..57])
    } else {
        query.to_string()
    };

    Some(Action {
        key: format!("websearch-{}", timestamp_minute()),
        content: format!("Web search: {}", display),
        fact_type: "context".to_string(),
        category: "research".to_string(),
    })
}

fn timestamp_minute() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() / 60)
        .unwrap_or(0)
}

fn should_save(key: &str) -> bool {
    let debounce = load_debounce();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    if let Some(last_save) = debounce.get(key).and_then(|v| v.as_u64()) {
        // Skip if saved within debounce window
        if now - last_save < DEBOUNCE_SECONDS {
            return false;
        }
    }

    true
}

fn mark_saved(key: &str) {
    let mut debounce = load_debounce();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    debounce[key] = serde_json::json!(now);

    // Clean old entries (older than 1 hour)
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

    // Save
    if let Ok(json) = serde_json::to_string(&debounce) {
        let _ = std::fs::write(DEBOUNCE_FILE, json);
    }
}

fn load_debounce() -> serde_json::Value {
    let path = Path::new(DEBOUNCE_FILE);
    if !path.exists() {
        return serde_json::json!({});
    }

    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::json!({}))
}

async fn save_action(session_id: &str, action: &Action) -> Result<()> {
    let mira_url = std::env::var("MIRA_URL")
        .unwrap_or_else(|_| "http://localhost:3000/mcp".to_string());
    let auth_token = std::env::var("MIRA_AUTH_TOKEN")
        .unwrap_or_else(|_| "63c7663fe0dbdfcd2bbf6c33a0a9b4b9".to_string());

    let client = reqwest::Client::new();

    // Initialize MCP session
    let init_req = McpRequest {
        jsonrpc: "2.0".to_string(),
        id: 1,
        method: "initialize".to_string(),
        params: serde_json::json!({
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": {"name": "posttool-hook", "version": "1.0"}
        }),
    };

    let resp = client
        .post(&mira_url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .header("Authorization", format!("Bearer {}", auth_token))
        .json(&init_req)
        .send()
        .await?;

    let mcp_session = resp
        .headers()
        .get("mcp-session-id")
        .and_then(|h| h.to_str().ok())
        .unwrap_or(session_id)
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
        .await?;

    // Call remember
    let remember_req = McpRequest {
        jsonrpc: "2.0".to_string(),
        id: 2,
        method: "tools/call".to_string(),
        params: serde_json::json!({
            "name": "remember",
            "arguments": {
                "content": action.content,
                "fact_type": action.fact_type,
                "category": action.category,
                "key": format!("auto-{}", action.key)
            }
        }),
    };

    client
        .post(&mira_url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .header("Authorization", format!("Bearer {}", auth_token))
        .header("Mcp-Session-Id", &mcp_session)
        .json(&remember_req)
        .send()
        .await?;

    Ok(())
}
