// src/operations/engine/tool_router/llm_conversation.rs
// Multi-turn LLM conversation executor for complex tool operations

use anyhow::{Context, Result};
use serde_json::{json, Value};
use tracing::{info, warn};

use crate::llm::provider::{Gemini3Provider, Message};
use super::super::file_handlers::FileHandlers;

/// Result of a multi-turn LLM conversation for file reading
pub struct FileReadResult {
    pub files: Vec<Value>,
    pub summary: String,
    pub tokens_input: i64,
    pub tokens_output: i64,
}

/// Execute a multi-turn LLM conversation for reading files
///
/// This handles the pattern of:
/// 1. Send prompt with tools to LLM
/// 2. Execute tool calls
/// 3. Add results to conversation
/// 4. Continue until LLM stops calling tools
pub async fn execute_file_read_conversation(
    llm: &Gemini3Provider,
    file_handlers: &FileHandlers,
    initial_messages: Vec<Message>,
    tools: Vec<Value>,
) -> Result<FileReadResult> {
    let mut response = llm
        .call_with_tools(initial_messages.clone(), tools.clone())
        .await
        .context("LLM file read failed")?;

    let mut all_files = Vec::new();
    let mut conversation = initial_messages;

    while !response.tool_calls.is_empty() {
        info!(
            "[ROUTER] LLM requested {} tool call(s)",
            response.tool_calls.len()
        );

        // Execute all tool calls
        let mut tool_results = Vec::new();
        for tool_call in &response.tool_calls {
            let result = file_handlers
                .execute_tool(&tool_call.name, tool_call.arguments.clone())
                .await;

            match result {
                Ok(res) => {
                    // Extract file content for aggregation
                    if let Some(content) = res.get("content").and_then(|c| c.as_str()) {
                        if let Some(path) = res.get("path").and_then(|p| p.as_str()) {
                            all_files.push(json!({
                                "path": path,
                                "content": content,
                                "lines": res.get("line_count"),
                                "chars": res.get("char_count")
                            }));
                        }
                    }
                    tool_results.push((tool_call.id.clone(), res));
                }
                Err(e) => {
                    warn!("[ROUTER] Tool execution failed: {}", e);
                    tool_results.push((
                        tool_call.id.clone(),
                        json!({
                            "success": false,
                            "error": e.to_string()
                        }),
                    ));
                }
            }
        }

        // Add assistant message with tool calls
        conversation.push(Message::assistant(
            response.content.clone().unwrap_or_default(),
        ));

        // Add tool results as user messages
        for (tool_id, result) in tool_results {
            conversation.push(Message::user(format!(
                "[Tool Result for {}]\n{}",
                tool_id,
                serde_json::to_string_pretty(&result).unwrap_or_default()
            )));
        }

        // Continue conversation with LLM
        response = llm
            .call_with_tools(conversation.clone(), tools.clone())
            .await
            .context("LLM continuation failed")?;

        // Break if LLM returns text instead of more tool calls
        if response.tool_calls.is_empty() {
            break;
        }
    }

    Ok(FileReadResult {
        files: all_files.clone(),
        summary: response.content.unwrap_or_else(|| {
            format!("Read {} files successfully", all_files.len())
        }),
        tokens_input: response.tokens_input,
        tokens_output: response.tokens_output,
    })
}
