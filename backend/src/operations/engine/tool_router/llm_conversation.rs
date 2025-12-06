// src/operations/engine/tool_router/llm_conversation.rs
// Multi-turn LLM conversation executor for complex tool operations

use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::sync::Arc;
use tracing::{info, warn};

use crate::llm::provider::{LlmProvider, Message, ToolCallInfo};
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
    llm: &Arc<dyn LlmProvider>,
    file_handlers: &FileHandlers,
    initial_messages: Vec<Message>,
    tools: Vec<Value>,
) -> Result<FileReadResult> {
    // Extract system message if present
    let system = initial_messages
        .iter()
        .find(|m| m.role == "system")
        .map(|m| m.content.clone())
        .unwrap_or_default();

    let mut response = llm
        .chat_with_tools(initial_messages.clone(), system.clone(), tools.clone(), None)
        .await
        .context("LLM file read failed")?;

    let mut all_files = Vec::new();
    let mut conversation = initial_messages;

    while !response.function_calls.is_empty() {
        info!(
            "[ROUTER] LLM requested {} tool call(s)",
            response.function_calls.len()
        );

        // Execute all tool calls
        let mut tool_results = Vec::new();
        for tool_call in &response.function_calls {
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
                    tool_results.push((tool_call.id.clone(), tool_call.name.clone(), res));
                }
                Err(e) => {
                    warn!("[ROUTER] Tool execution failed: {}", e);
                    tool_results.push((
                        tool_call.id.clone(),
                        tool_call.name.clone(),
                        json!({
                            "success": false,
                            "error": e.to_string()
                        }),
                    ));
                }
            }
        }

        // Add assistant message with tool calls
        let tool_calls_info: Vec<ToolCallInfo> = response.function_calls.iter().map(|tc| {
            ToolCallInfo {
                id: tc.id.clone(),
                name: tc.name.clone(),
                arguments: tc.arguments.clone(),
            }
        }).collect();

        conversation.push(Message::assistant_with_tool_calls(
            response.text_output.clone(),
            tool_calls_info,
        ));

        // Add tool results as tool_result messages
        for (tool_id, tool_name, result) in tool_results {
            conversation.push(Message::tool_result(
                tool_id,
                tool_name,
                serde_json::to_string_pretty(&result).unwrap_or_default(),
            ));
        }

        // Continue conversation with LLM
        response = llm
            .chat_with_tools(conversation.clone(), system.clone(), tools.clone(), None)
            .await
            .context("LLM continuation failed")?;

        // Break if LLM returns text instead of more tool calls
        if response.function_calls.is_empty() {
            break;
        }
    }

    Ok(FileReadResult {
        files: all_files.clone(),
        summary: if response.text_output.is_empty() {
            format!("Read {} files successfully", all_files.len())
        } else {
            response.text_output
        },
        tokens_input: response.tokens.input,
        tokens_output: response.tokens.output,
    })
}
