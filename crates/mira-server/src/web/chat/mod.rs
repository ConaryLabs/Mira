// web/chat/mod.rs
// Chat API handlers (DeepSeek Reasoner)

mod context;
mod extraction;
mod summarization;
mod tools;

pub use summarization::get_summary_context;

use axum::{
    extract::State,
    Json,
};
use mira_types::{ChatRequest, ChatUsage, WsEvent};
use std::time::Instant;
use tracing::{error, info, instrument, warn};

use context::build_system_prompt;
use extraction::spawn_fact_extraction;
use summarization::maybe_spawn_summarization;
use tools::execute_tools;

use crate::web::deepseek::{Message, mira_tools};
use crate::web::state::AppState;

/// Chat with DeepSeek Reasoner
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

    // Broadcast chat start
    state.broadcast(WsEvent::ChatStart {
        message: req.message.clone(),
    });

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

    // Call DeepSeek
    match deepseek.chat(messages, Some(tools)).await {
        Ok(result) => {
            // Handle tool calls if present
            if let Some(tool_calls) = &result.tool_calls {
                // Execute tools and continue conversation
                let _tool_results = execute_tools(&state, tool_calls).await;

                // For now, return the partial result - full tool loop TBD
                info!("Chat completed with {} tool calls", tool_calls.len());
            }

            // Broadcast completion
            let usage = result.usage.map(|u| ChatUsage {
                prompt_tokens: u.prompt_tokens,
                completion_tokens: u.completion_tokens,
                total_tokens: u.total_tokens,
                cache_hit_tokens: u.prompt_cache_hit_tokens,
                cache_miss_tokens: u.prompt_cache_miss_tokens,
            });

            let response_content = result.content.clone().unwrap_or_default();

            state.broadcast(WsEvent::ChatComplete {
                content: response_content.clone(),
                model: "deepseek-reasoner".to_string(),
                usage,
            });

            // Store assistant response in history
            // Use content if available, fall back to reasoning_content
            let assistant_content = if !response_content.is_empty() {
                response_content.clone()
            } else {
                result.reasoning_content.clone().unwrap_or_default()
            };
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

            Json(mira_types::ApiResponse::ok(serde_json::json!({
                "content": result.content,
                "reasoning_content": result.reasoning_content,
                "tool_calls": result.tool_calls,
            })))
        }
        Err(e) => {
            state.broadcast(WsEvent::ChatError {
                message: e.to_string(),
            });
            Json(mira_types::ApiResponse::err(e.to_string()))
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
            // Use content if available, fall back to reasoning_content (reasoner quirk)
            let assistant_content = result.content.clone()
                .or_else(|| result.reasoning_content.clone())
                .unwrap_or_default();
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
