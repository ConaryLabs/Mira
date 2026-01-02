// web/chat/mod.rs
// Chat API handlers (DeepSeek Reasoner)

mod context;
mod extraction;
pub mod stream;
mod summarization;
mod tools;

pub use stream::chat_stream;
pub use summarization::get_summary_context;

use axum::{
    extract::State,
    Json,
};
use mira_types::ChatRequest;
use std::time::Instant;
use tracing::{error, info, instrument, warn};

use context::build_system_prompt;
use extraction::spawn_fact_extraction;
use summarization::maybe_spawn_summarization;
use tools::execute_tools;

use crate::web::deepseek::{Message, mira_tools};
use crate::web::state::AppState;

/// Clean up response content by removing JSON garbage
/// (model sometimes outputs JSON fragments during tool calls)
pub fn cleanup_response(content: String) -> String {
    let mut result = content.trim().to_string();

    // Remove trailing brackets/braces
    while result.ends_with("[]") || result.ends_with("{}") {
        result = result[..result.len()-2].trim().to_string();
    }
    while result.ends_with('[') || result.ends_with(']')
        || result.ends_with('{') || result.ends_with('}') {
        result = result[..result.len()-1].trim().to_string();
    }

    // Remove leading brackets/braces
    while result.starts_with("[]") || result.starts_with("{}") {
        result = result[2..].trim().to_string();
    }
    while result.starts_with('[') || result.starts_with(']')
        || result.starts_with('{') || result.starts_with('}') {
        result = result[1..].trim().to_string();
    }

    // Remove fact-extraction JSON: [ {"content":..., "category":...} ]
    for (i, c) in result.clone().char_indices().rev() {
        if c == '[' {
            let trailing = &result[i..];
            if trailing.contains("\"content\":") && trailing.contains("\"category\":") {
                result = result[..i].trim().to_string();
                break;
            }
        }
    }

    // If result is just JSON-like punctuation, return empty
    if result.chars().all(|c| matches!(c, '[' | ']' | '{' | '}' | ',' | ':' | '"' | ' ' | '\n' | '\t')) {
        return String::new();
    }

    result
}

/// Chat with DeepSeek Reasoner (non-streaming HTTP endpoint)
/// For streaming, use /api/chat/stream instead
pub async fn chat(
    State(state): State<AppState>,
    Json(req): Json<ChatRequest>,
) -> Json<mira_types::ApiResponse<serde_json::Value>> {
    let deepseek = match &state.deepseek {
        Some(ds) => ds,
        None => {
            return Json(mira_types::ApiResponse::err(
                "DeepSeek not configured. Set DEEPSEEK_API_KEY environment variable.",
            ))
        }
    };

    // Store user message in history
    if let Err(e) = state.db.store_chat_message("user", &req.message, None) {
        warn!("Failed to store user message: {}", e);
    }

    // Build messages with system prompt (includes personal context based on user message)
    let mut messages = vec![Message::system(build_system_prompt(&state, &req.message).await)];

    // Add stored conversation history (recent messages from DB)
    // This gives continuity across page refreshes / sessions
    if req.history.is_empty() {
        // No client-side history, load from DB
        if let Ok(recent) = state.db.get_recent_messages(20) {
            for msg in recent {
                messages.push(Message {
                    role: msg.role,
                    content: Some(msg.content),
                    reasoning_content: msg.reasoning_content,
                    tool_calls: None,
                    tool_call_id: None,
                });
            }
        }
    } else {
        // Use client-provided history
        for msg in &req.history {
            let role = match msg.role {
                mira_types::ChatRole::System => "system",
                mira_types::ChatRole::User => "user",
                mira_types::ChatRole::Assistant => "assistant",
                mira_types::ChatRole::Tool => "tool",
            };
            messages.push(Message {
                role: role.to_string(),
                content: msg.content.clone(),
                reasoning_content: msg.reasoning_content.clone(),
                tool_calls: None,
                tool_call_id: msg.tool_call_id.clone(),
            });
        }
    }

    // Add user message
    messages.push(Message::user(&req.message));

    // Get tools
    let tools = mira_tools();

    // Tool call loop - continue until we get a final response
    const MAX_TOOL_ROUNDS: usize = 8;
    let mut current_messages = messages;
    let mut final_result = None;
    let mut last_result = None; // Keep track of last result for fallback

    for round in 0..MAX_TOOL_ROUNDS {
        // Call DeepSeek
        match deepseek.chat(current_messages.clone(), Some(tools.clone())).await {
            Ok(result) => {
                // Check if we have tool calls to process
                if let Some(ref tool_calls) = result.tool_calls {
                    if tool_calls.is_empty() {
                        final_result = Some(result);
                        break;
                    }

                    info!("Tool round {}: {} tool calls", round + 1, tool_calls.len());

                    // Save this result as fallback in case we exhaust rounds
                    last_result = Some(result.clone());

                    // Execute tools
                    let tool_results = execute_tools(&state, tool_calls).await;

                    // Add assistant message with tool calls to history
                    current_messages.push(Message {
                        role: "assistant".to_string(),
                        content: result.content.clone(),
                        reasoning_content: result.reasoning_content.clone(),
                        tool_calls: Some(tool_calls.clone()),
                        tool_call_id: None,
                    });

                    // Add tool result messages
                    for (call_id, result_content) in tool_results {
                        current_messages.push(Message::tool_result(call_id, result_content));
                    }

                    // Continue to next round
                } else {
                    // No tool calls, we're done
                    final_result = Some(result);
                    break;
                }
            }
            Err(e) => {
                return Json(mira_types::ApiResponse::err(e.to_string()));
            }
        }
    }

    // Handle final result
    match final_result {
        Some(result) => {
            let raw_content = result.content.clone().unwrap_or_default();
            let response_content = cleanup_response(raw_content.clone());

            if raw_content != response_content {
                info!("Cleaned up response: {} chars -> {} chars", raw_content.len(), response_content.len());
            }

            // Store assistant response in history
            if !response_content.is_empty() {
                let assistant_content = response_content.clone();
                if let Err(e) = state.db.store_chat_message(
                    "assistant",
                    &assistant_content,
                    result.reasoning_content.as_deref(),
                ) {
                    warn!("Failed to store assistant message: {}", e);
                }

                // Spawn background tasks (non-blocking)
                spawn_fact_extraction(
                    state.clone(),
                    req.message.clone(),
                    assistant_content,
                );

                // Check if we need to roll up older messages into summaries
                maybe_spawn_summarization(state.clone());
            }

            Json(mira_types::ApiResponse::ok(serde_json::json!({
                "content": response_content,
                "reasoning_content": result.reasoning_content,
                "tool_calls": result.tool_calls,
            })))
        }
        None => {
            // Exhausted tool rounds - use last result if it has content
            if let Some(result) = last_result {
                let content = result.content.unwrap_or_default();
                if !content.is_empty() {
                    let response_content = cleanup_response(content);
                    return Json(mira_types::ApiResponse::ok(serde_json::json!({
                        "content": response_content,
                        "note": "Response after max tool rounds",
                    })));
                }
            }

            // No usable content
            Json(mira_types::ApiResponse::ok(serde_json::json!({
                "content": "I got a bit carried away with tools there. Could you rephrase your question?",
            })))
        }
    }
}

/// Test chat endpoint - returns detailed JSON for debugging
/// Used by `mira test-chat` CLI and for programmatic testing
#[instrument(skip(state, req), fields(message_len = req.message.len()))]
pub async fn test_chat(
    State(state): State<AppState>,
    Json(req): Json<ChatRequest>,
) -> Json<mira_types::ApiResponse<serde_json::Value>> {
    let start_time = Instant::now();

    let deepseek = match &state.deepseek {
        Some(ds) => ds,
        None => {
            return Json(mira_types::ApiResponse::err(
                "DeepSeek not configured. Set DEEPSEEK_API_KEY environment variable.",
            ))
        }
    };

    info!(message = %req.message, "Test chat request received");

    // Store user message in history
    if let Err(e) = state.db.store_chat_message("user", &req.message, None) {
        warn!("Failed to store user message: {}", e);
    }

    // Build messages (includes personal context based on user message)
    let mut messages = vec![Message::system(build_system_prompt(&state, &req.message).await)];

    // Add stored conversation history (recent messages from DB)
    if req.history.is_empty() {
        if let Ok(recent) = state.db.get_recent_messages(20) {
            for msg in recent {
                messages.push(Message {
                    role: msg.role,
                    content: Some(msg.content),
                    reasoning_content: msg.reasoning_content,
                    tool_calls: None,
                    tool_call_id: None,
                });
            }
        }
    } else {
        for msg in &req.history {
            let role = match msg.role {
                mira_types::ChatRole::System => "system",
                mira_types::ChatRole::User => "user",
                mira_types::ChatRole::Assistant => "assistant",
                mira_types::ChatRole::Tool => "tool",
            };
            messages.push(Message {
                role: role.to_string(),
                content: msg.content.clone(),
                reasoning_content: msg.reasoning_content.clone(),
                tool_calls: None,
                tool_call_id: msg.tool_call_id.clone(),
            });
        }
    }

    messages.push(Message::user(&req.message));

    let tools = mira_tools();
    let tool_names: Vec<String> = tools.iter().map(|t| t.function.name.clone()).collect();

    // Call DeepSeek
    match deepseek.chat(messages, Some(tools)).await {
        Ok(result) => {
            let duration_ms = start_time.elapsed().as_millis() as u64;

            // Execute tools if requested
            let mut tool_results = Vec::new();
            if let Some(ref tool_calls) = result.tool_calls {
                let results = execute_tools(&state, tool_calls).await;
                for (id, res) in results {
                    tool_results.push(serde_json::json!({
                        "call_id": id,
                        "result": res,
                    }));
                }
            }

            let response = serde_json::json!({
                "success": true,
                "request_id": result.request_id,
                "duration_ms": duration_ms,
                "deepseek_duration_ms": result.duration_ms,
                "content": result.content,
                "reasoning_content": result.reasoning_content,
                "tool_calls": result.tool_calls,
                "tool_results": tool_results,
                "usage": result.usage.map(|u| serde_json::json!({
                    "prompt_tokens": u.prompt_tokens,
                    "completion_tokens": u.completion_tokens,
                    "total_tokens": u.total_tokens,
                    "cache_hit_tokens": u.prompt_cache_hit_tokens,
                    "cache_miss_tokens": u.prompt_cache_miss_tokens,
                })),
                "available_tools": tool_names,
            });

            info!(
                request_id = %result.request_id,
                duration_ms = duration_ms,
                "Test chat complete"
            );

            // Store assistant response and spawn background tasks
            // Only store actual content, not reasoning (reasoning stored separately)
            let assistant_content = cleanup_response(result.content.clone().unwrap_or_default());
            if !assistant_content.is_empty() {
                if let Err(e) = state.db.store_chat_message(
                    "assistant",
                    &assistant_content,
                    result.reasoning_content.as_deref(),
                ) {
                    warn!("Failed to store assistant message: {}", e);
                }

                // Spawn background tasks (non-blocking)
                spawn_fact_extraction(
                    state.clone(),
                    req.message.clone(),
                    assistant_content,
                );

                // Check if we need to roll up older messages into summaries
                maybe_spawn_summarization(state.clone());
            }

            Json(mira_types::ApiResponse::ok(response))
        }
        Err(e) => {
            error!(error = %e, "Test chat failed");
            Json(mira_types::ApiResponse::err(e.to_string()))
        }
    }
}
