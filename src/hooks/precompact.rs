//! PreCompact hook for Claude Code
//!
//! Fires before conversation compaction to save context to Mira.
//! Extracts files modified, decisions, topics from the transcript
//! and saves via HTTP API so embeddings are generated.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::{self, BufRead, Read};
use std::path::Path;

#[derive(Debug, Deserialize)]
struct HookInput {
    hook_event_name: String,
    session_id: Option<String>,
    transcript_path: Option<String>,
    trigger: Option<String>,
}

#[derive(Debug, Default)]
struct TranscriptContext {
    files_modified: HashSet<String>,
    files_read: HashSet<String>,
    decisions: Vec<String>,
    topics: HashSet<String>,
    tool_calls: Vec<ToolCall>,
    user_requests: Vec<String>,
    errors: Vec<String>,
}

#[derive(Debug)]
#[allow(dead_code)] // Used for debug output
struct ToolCall {
    tool: String,
    input_summary: String,
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
    status: String,
}

// MCP types for HTTP API calls
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

    // Only process PreCompact events
    if hook_input.hook_event_name != "PreCompact" {
        return Ok(());
    }

    let session_id = hook_input.session_id.unwrap_or_else(|| "unknown".to_string());
    let transcript_path = match hook_input.transcript_path {
        Some(p) => p,
        None => return Ok(()),
    };
    let trigger = hook_input.trigger.unwrap_or_else(|| "unknown".to_string());

    // Extract context from transcript
    let context = extract_transcript_context(&transcript_path);

    // Generate summary
    let summary = generate_summary(&context, &trigger);

    // Save to Mira via HTTP API
    let result = save_to_mira(&session_id, &trigger, &context, &summary).await;

    // Output status
    if let Ok(snapshot_id) = result {
        let output = HookOutput {
            hook_specific_output: HookSpecificOutput {
                hook_event_name: "PreCompact".to_string(),
                status: format!("Saved pre-compaction context to Mira (snapshot: {})", snapshot_id),
            },
        };
        println!("{}", serde_json::to_string(&output).unwrap());
    }

    Ok(())
}

fn extract_transcript_context(transcript_path: &str) -> TranscriptContext {
    let mut context = TranscriptContext::default();

    let path = Path::new(transcript_path);
    if !path.exists() {
        return context;
    }

    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return context,
    };

    let reader = io::BufReader::new(file);
    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };

        if line.trim().is_empty() {
            continue;
        }

        let entry: serde_json::Value = match serde_json::from_str(&line) {
            Ok(e) => e,
            Err(_) => continue,
        };

        process_transcript_entry(&entry, &mut context);
    }

    context
}

fn process_transcript_entry(entry: &serde_json::Value, context: &mut TranscriptContext) {
    let msg_type = entry.get("type").and_then(|t| t.as_str()).unwrap_or("");

    match msg_type {
        "user" => {
            if let Some(content) = entry
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_str())
            {
                if !content.is_empty() {
                    // Truncate long messages
                    let truncated: String = content.chars().take(500).collect();
                    context.user_requests.push(truncated);
                    extract_topics(content, &mut context.topics);
                }
            }
        }
        "assistant" => {
            if let Some(content) = entry
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_array())
            {
                for block in content {
                    if let Some(block_type) = block.get("type").and_then(|t| t.as_str()) {
                        match block_type {
                            "tool_use" => {
                                let tool_name = block
                                    .get("name")
                                    .and_then(|n| n.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                let tool_input = block.get("input").cloned().unwrap_or(serde_json::json!({}));

                                let input_summary = summarize_tool_input(&tool_name, &tool_input);
                                context.tool_calls.push(ToolCall {
                                    tool: tool_name.clone(),
                                    input_summary,
                                });

                                // Track file operations
                                match tool_name.as_str() {
                                    "Edit" | "Write" => {
                                        if let Some(path) = tool_input.get("file_path").and_then(|p| p.as_str()) {
                                            context.files_modified.insert(path.to_string());
                                        }
                                    }
                                    "Read" => {
                                        if let Some(path) = tool_input.get("file_path").and_then(|p| p.as_str()) {
                                            context.files_read.insert(path.to_string());
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            "text" => {
                                if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                    extract_decisions(text, &mut context.decisions);
                                    extract_topics(text, &mut context.topics);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        "tool_result" => {
            if let Some(result) = entry.get("result") {
                if result.get("is_error").and_then(|e| e.as_bool()).unwrap_or(false) {
                    if let Some(content) = result.get("content").and_then(|c| c.as_str()) {
                        let truncated: String = content.chars().take(200).collect();
                        context.errors.push(truncated);
                    }
                }
            }
        }
        _ => {}
    }
}

fn summarize_tool_input(tool_name: &str, tool_input: &serde_json::Value) -> String {
    match tool_name {
        "Bash" => tool_input
            .get("command")
            .and_then(|c| c.as_str())
            .map(|s| s.chars().take(100).collect())
            .unwrap_or_default(),
        "Edit" | "Write" | "Read" => tool_input
            .get("file_path")
            .and_then(|p| p.as_str())
            .map(|s| s.chars().take(100).collect())
            .unwrap_or_default(),
        "Grep" | "Glob" => {
            let pattern = tool_input
                .get("pattern")
                .and_then(|p| p.as_str())
                .unwrap_or("");
            format!("pattern: {}", pattern.chars().take(50).collect::<String>())
        }
        _ => {
            let s = tool_input.to_string();
            s.chars().take(100).collect()
        }
    }
}

fn extract_decisions(text: &str, decisions: &mut Vec<String>) {
    // Fallback pattern matching for decision-like statements
    // Used when LLM extraction is not available
    let patterns = [
        "I'll ", "I will ", "Let's ", "We should ", "Going to ",
        "I'm going to ", "I decided to ", "The approach is to ",
        "Using ", "Switching to ", "Implementing ", "Creating ", "Adding ",
    ];

    for line in text.lines() {
        for pattern in &patterns {
            if let Some(idx) = line.to_lowercase().find(&pattern.to_lowercase()) {
                let start = idx + pattern.len();
                if start < line.len() {
                    let rest: String = line[start..].chars().take(150).collect();
                    if rest.len() > 10 && decisions.len() < 20 && !decisions.contains(&rest) {
                        decisions.push(rest);
                    }
                }
            }
        }
    }
}

fn extract_topics(text: &str, topics: &mut HashSet<String>) {
    let keywords = [
        "api", "database", "authentication", "auth", "testing", "test",
        "deployment", "docker", "kubernetes", "git", "ci/cd", "pipeline",
        "frontend", "backend", "server", "client", "ui", "ux",
        "bug", "fix", "feature", "refactor", "optimization", "performance",
        "security", "encryption", "migration", "config", "configuration",
        "rust", "python", "typescript", "javascript", "sql", "json",
        "mcp", "embeddings", "qdrant", "semantic", "indexer", "daemon",
        "hook", "permission", "compaction",
    ];

    let text_lower = text.to_lowercase();
    for keyword in &keywords {
        if text_lower.contains(keyword) {
            topics.insert(keyword.to_string());
        }
    }

    // Limit topics
    while topics.len() > 30 {
        if let Some(topic) = topics.iter().next().cloned() {
            topics.remove(&topic);
        }
    }
}

fn generate_summary(context: &TranscriptContext, trigger: &str) -> String {
    let mut parts = vec![
        format!("[Pre-Compaction Save - {}]", trigger),
        format!("Compaction triggered: {}", trigger),
    ];

    if !context.files_modified.is_empty() {
        parts.push(format!("\nFiles modified ({}):", context.files_modified.len()));
        for (i, f) in context.files_modified.iter().enumerate() {
            if i >= 10 {
                parts.push(format!("  ... and {} more", context.files_modified.len() - 10));
                break;
            }
            // Show relative paths where possible
            let display = if f.contains("/Mira/") {
                f.split("/Mira/").last().unwrap_or(f)
            } else {
                f.split('/').next_back().unwrap_or(f)
            };
            parts.push(format!("  - {}", display));
        }
    }

    if !context.user_requests.is_empty() {
        parts.push(format!("\nUser requests ({}):", context.user_requests.len()));
        for (i, req) in context.user_requests.iter().enumerate() {
            if i >= 5 {
                break;
            }
            let first_line: String = req.lines().next().unwrap_or("").chars().take(100).collect();
            parts.push(format!("  - {}", first_line));
        }
    }

    if !context.decisions.is_empty() {
        parts.push("\nKey decisions/actions:".to_string());
        for (i, dec) in context.decisions.iter().enumerate() {
            if i >= 10 {
                break;
            }
            parts.push(format!("  - {}", dec));
        }
    }

    if !context.topics.is_empty() {
        let topics_str: Vec<_> = context.topics.iter().cloned().collect();
        parts.push(format!("\nTopics: {}", topics_str.join(", ")));
    }

    if !context.tool_calls.is_empty() {
        let mut tool_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for tc in &context.tool_calls {
            *tool_counts.entry(tc.tool.clone()).or_insert(0) += 1;
        }
        let tools_str: Vec<_> = tool_counts
            .iter()
            .map(|(k, v)| format!("{}({})", k, v))
            .collect();
        parts.push(format!("\nTools used: {}", tools_str.join(", ")));
    }

    if !context.errors.is_empty() {
        parts.push(format!("\nErrors encountered: {}", context.errors.len()));
    }

    parts.join("\n")
}

/// Extract decisions using LLM via MCP extract tool
/// Falls back to pattern matching if LLM extraction fails
async fn extract_decisions_llm(
    client: &reqwest::Client,
    mira_url: &str,
    auth_token: &str,
    mcp_session: &str,
    transcript_content: &str,
) -> Option<(Vec<String>, Vec<String>, Vec<String>)> {
    // Skip if transcript is too small
    if transcript_content.len() < 100 {
        return None;
    }

    // Truncate transcript for LLM (first 3000 chars)
    let truncated: String = transcript_content.chars().take(3000).collect();

    let req = McpRequest {
        jsonrpc: "2.0".to_string(),
        id: 10,
        method: "tools/call".to_string(),
        params: serde_json::json!({
            "name": "extract",
            "arguments": {
                "transcript": truncated
            }
        }),
    };

    let resp = client
        .post(mira_url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .header("Authorization", format!("Bearer {}", auth_token))
        .header("Mcp-Session-Id", mcp_session)
        .json(&req)
        .send()
        .await
        .ok()?;

    let body = resp.text().await.ok()?;

    // Parse SSE response for decisions, topics, insights
    let mut decisions = Vec::new();
    let mut topics = Vec::new();
    let mut insights = Vec::new();

    // Look for JSON in SSE response
    for line in body.lines() {
        if line.contains("\"decisions\"") {
            // Try to parse decisions array
            if let Some(start) = line.find("\"decisions\"") {
                if let Some(arr_start) = line[start..].find('[') {
                    let rest = &line[start + arr_start..];
                    if let Some(end) = find_matching_bracket(rest) {
                        let arr_str = &rest[..end + 1];
                        if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(arr_str) {
                            for d in arr {
                                if let Some(content) = d.get("content").and_then(|c| c.as_str()) {
                                    if d.get("confidence").and_then(|c| c.as_f64()).unwrap_or(0.5) >= 0.6 {
                                        decisions.push(content.to_string());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        if line.contains("\"topics\"") {
            if let Some(start) = line.find("\"topics\"") {
                if let Some(arr_start) = line[start..].find('[') {
                    let rest = &line[start + arr_start..];
                    if let Some(end) = find_matching_bracket(rest) {
                        let arr_str = &rest[..end + 1];
                        if let Ok(arr) = serde_json::from_str::<Vec<String>>(arr_str) {
                            topics = arr;
                        }
                    }
                }
            }
        }
        if line.contains("\"insights\"") {
            if let Some(start) = line.find("\"insights\"") {
                if let Some(arr_start) = line[start..].find('[') {
                    let rest = &line[start + arr_start..];
                    if let Some(end) = find_matching_bracket(rest) {
                        let arr_str = &rest[..end + 1];
                        if let Ok(arr) = serde_json::from_str::<Vec<String>>(arr_str) {
                            insights = arr;
                        }
                    }
                }
            }
        }
    }

    if decisions.is_empty() && topics.is_empty() && insights.is_empty() {
        return None;
    }

    Some((decisions, topics, insights))
}

/// Find matching bracket in string
fn find_matching_bracket(s: &str) -> Option<usize> {
    let mut depth = 0;
    for (i, c) in s.chars().enumerate() {
        match c {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

async fn save_to_mira(
    session_id: &str,
    trigger: &str,
    context: &TranscriptContext,
    summary: &str,
) -> Result<String> {
    let mira_url = std::env::var("MIRA_URL").unwrap_or_else(|_| "http://localhost:3000/mcp".to_string());
    let auth_token = std::env::var("MIRA_AUTH_TOKEN").unwrap_or_else(|_| "63c7663fe0dbdfcd2bbf6c33a0a9b4b9".to_string());

    let client = reqwest::Client::new();

    // Generate snapshot ID
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let snapshot_id = format!("{:x}", md5::compute(format!("{}-{}", session_id, timestamp)));
    let snapshot_id = &snapshot_id[..16];

    // Initialize MCP session
    let init_req = McpRequest {
        jsonrpc: "2.0".to_string(),
        id: 1,
        method: "initialize".to_string(),
        params: serde_json::json!({
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "precompact-hook-rust", "version": "1.0"}
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

    // Try LLM extraction for better decision/topic extraction
    // Collect raw transcript text for LLM
    let transcript_text: String = context.user_requests.iter()
        .chain(context.decisions.iter())
        .take(50)
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");

    let (llm_decisions, llm_topics, llm_insights) = match extract_decisions_llm(
        &client,
        &mira_url,
        &auth_token,
        &mcp_session,
        &transcript_text,
    ).await {
        Some((d, t, i)) => (Some(d), Some(t), Some(i)),
        None => (None, None, None),
    };

    // Store session summary
    let full_summary = format!("[Pre-Compaction Save - {}]\n{}", trigger, summary);
    // Use LLM topics if available, otherwise fall back to pattern-matched
    let topics: Vec<_> = llm_topics.clone()
        .unwrap_or_else(|| context.topics.iter().take(10).cloned().collect());

    call_tool(
        &client,
        &mira_url,
        &auth_token,
        &mcp_session,
        "store_session",
        serde_json::json!({
            "summary": full_summary,
            "session_id": session_id,
            "topics": topics
        }),
    )
    .await?;

    // Store files modified (confidence 0.8 for compaction summaries)
    if !context.files_modified.is_empty() {
        let files: Vec<_> = context.files_modified.iter().take(20).cloned().collect();
        let content = format!("Files modified before compaction ({}): {}", trigger, files.join(", "));

        call_tool(
            &client,
            &mira_url,
            &auth_token,
            &mcp_session,
            "remember",
            serde_json::json!({
                "content": content,
                "fact_type": "context",
                "category": "compaction",
                "key": format!("compaction-files-{}", snapshot_id),
                "confidence": 0.8
            }),
        )
        .await?;
    }

    // Store decisions - use LLM-extracted if available (higher confidence)
    let decisions = llm_decisions.clone()
        .unwrap_or_else(|| context.decisions.iter().take(15).cloned().collect());
    let decision_confidence = if llm_decisions.is_some() { 0.9 } else { 0.8 };

    if !decisions.is_empty() {
        let content = format!(
            "Decisions made before compaction ({}):\n{}",
            trigger,
            decisions.iter().map(|d| format!("- {}", d)).collect::<Vec<_>>().join("\n")
        );

        call_tool(
            &client,
            &mira_url,
            &auth_token,
            &mcp_session,
            "remember",
            serde_json::json!({
                "content": content,
                "fact_type": "decision",
                "category": "compaction",
                "key": format!("compaction-decisions-{}", snapshot_id),
                "confidence": decision_confidence
            }),
        )
        .await?;
    }

    // Store LLM insights if available (new capability)
    if let Some(insights) = llm_insights {
        if !insights.is_empty() {
            let content = format!(
                "Session insights (LLM-extracted):\n{}",
                insights.iter().map(|i| format!("- {}", i)).collect::<Vec<_>>().join("\n")
            );

            call_tool(
                &client,
                &mira_url,
                &auth_token,
                &mcp_session,
                "remember",
                serde_json::json!({
                    "content": content,
                    "fact_type": "context",
                    "category": "insights",
                    "key": format!("compaction-insights-{}", snapshot_id),
                    "confidence": 0.85
                }),
            )
            .await?;
        }
    }

    // Store user requests (confidence 0.8 for compaction summaries)
    if !context.user_requests.is_empty() {
        let requests: Vec<_> = context
            .user_requests
            .iter()
            .take(10)
            .map(|r| r.chars().take(150).collect::<String>())
            .collect();
        let content = format!(
            "User requests before compaction ({}):\n{}",
            trigger,
            requests.iter().map(|r| format!("- {}", r)).collect::<Vec<_>>().join("\n")
        );

        call_tool(
            &client,
            &mira_url,
            &auth_token,
            &mcp_session,
            "remember",
            serde_json::json!({
                "content": content,
                "fact_type": "context",
                "category": "compaction",
                "key": format!("compaction-requests-{}", snapshot_id),
                "confidence": 0.8
            }),
        )
        .await?;
    }

    Ok(snapshot_id.to_string())
}

async fn call_tool(
    client: &reqwest::Client,
    mira_url: &str,
    auth_token: &str,
    session_id: &str,
    tool_name: &str,
    arguments: serde_json::Value,
) -> Result<()> {
    let req = McpRequest {
        jsonrpc: "2.0".to_string(),
        id: 2,
        method: "tools/call".to_string(),
        params: serde_json::json!({
            "name": tool_name,
            "arguments": arguments
        }),
    };

    client
        .post(mira_url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .header("Authorization", format!("Bearer {}", auth_token))
        .header("Mcp-Session-Id", session_id)
        .json(&req)
        .send()
        .await?;

    Ok(())
}
