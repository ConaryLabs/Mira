// web/chat/stream.rs
// SSE streaming chat endpoint

use axum::{
    extract::State,
    response::sse::{Event, KeepAlive, Sse},
    Json,
};
use futures::stream::Stream;
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use tokio::sync::mpsc;
use tracing::{info, warn};

use super::context::build_system_prompt;
use super::extraction::spawn_fact_extraction;
use super::summarization::maybe_spawn_summarization;
use super::tools::execute_tools;
use super::cleanup_response;

use crate::web::deepseek::{Message, ToolCall, mira_tools};
use crate::web::state::AppState;

/// SSE event types for chat streaming
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatEvent {
    /// Stream is starting
    Start,
    /// Text content delta
    Delta { content: String },
    /// Reasoning content (not streamed, sent at end)
    Reasoning { content: String },
    /// Tool call starting
    ToolStart { name: String, call_id: String },
    /// Tool call completed
    ToolResult { name: String, call_id: String, success: bool },
    /// Chat complete with final content
    Done { content: String },
    /// Error occurred
    Error { message: String },
}

/// Request for streaming chat
#[derive(Debug, Deserialize)]
pub struct StreamChatRequest {
    pub message: String,
}

/// Streaming chat endpoint - returns SSE stream
pub async fn chat_stream(
    State(state): State<AppState>,
    Json(req): Json<StreamChatRequest>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let (tx, rx) = mpsc::channel::<ChatEvent>(100);

    // Spawn the chat processing task
    let state_clone = state.clone();
    let message = req.message.clone();
    tokio::spawn(async move {
        process_chat_stream(state_clone, message, tx).await;
    });

    // Convert channel to SSE stream
    let stream = async_stream::stream! {
        let mut rx = rx;
        while let Some(event) = rx.recv().await {
            let data = serde_json::to_string(&event).unwrap_or_default();
            yield Ok(Event::default().data(data));
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// Process chat and send events to channel
async fn process_chat_stream(
    state: AppState,
    user_message: String,
    tx: mpsc::Sender<ChatEvent>,
) {
    // Send start event
    let _ = tx.send(ChatEvent::Start).await;

    let deepseek = match &state.deepseek {
        Some(ds) => ds,
        None => {
            let _ = tx.send(ChatEvent::Error {
                message: "DeepSeek not configured".to_string(),
            }).await;
            return;
        }
    };

    // Store user message
    if let Err(e) = state.db.store_chat_message("user", &user_message, None) {
        warn!("Failed to store user message: {}", e);
    }

    // Build messages with system prompt
    let mut messages = vec![Message::system(build_system_prompt(&state, &user_message).await)];

    // Load conversation history from DB
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

    // Add user message
    messages.push(Message::user(&user_message));

    // Get tools
    let tools = mira_tools();

    // Tool call loop
    const MAX_TOOL_ROUNDS: usize = 8;
    let mut current_messages = messages;
    let mut final_content = String::new();
    let mut final_reasoning: Option<String> = None;

    for round in 0..MAX_TOOL_ROUNDS {
        // Call DeepSeek with channel-based streaming
        match deepseek.chat_to_channel(current_messages.clone(), Some(tools.clone()), tx.clone()).await {
            Ok(result) => {
                // Check if we have tool calls
                if let Some(ref tool_calls) = result.tool_calls {
                    if tool_calls.is_empty() {
                        final_content = result.content.unwrap_or_default();
                        final_reasoning = result.reasoning_content;
                        break;
                    }

                    info!("Tool round {}: {} tool calls", round + 1, tool_calls.len());

                    // Execute tools and send events
                    for tc in tool_calls {
                        let _ = tx.send(ChatEvent::ToolStart {
                            name: tc.function.name.clone(),
                            call_id: tc.id.clone(),
                        }).await;
                    }

                    let tool_results = execute_tools(&state, tool_calls).await;

                    // Send tool result events
                    for (call_id, _) in &tool_results {
                        if let Some(tc) = tool_calls.iter().find(|t| &t.id == call_id) {
                            let _ = tx.send(ChatEvent::ToolResult {
                                name: tc.function.name.clone(),
                                call_id: call_id.clone(),
                                success: true,
                            }).await;
                        }
                    }

                    // Add assistant message with tool calls
                    current_messages.push(Message {
                        role: "assistant".to_string(),
                        content: result.content.clone(),
                        reasoning_content: result.reasoning_content.clone(),
                        tool_calls: Some(tool_calls.clone()),
                        tool_call_id: None,
                    });

                    // Add tool results
                    for (call_id, result_content) in tool_results {
                        current_messages.push(Message::tool_result(call_id, result_content));
                    }
                } else {
                    // No tool calls, done
                    final_content = result.content.unwrap_or_default();
                    final_reasoning = result.reasoning_content;
                    break;
                }
            }
            Err(e) => {
                let _ = tx.send(ChatEvent::Error {
                    message: e.to_string(),
                }).await;
                return;
            }
        }
    }

    // Clean up response
    let cleaned_content = cleanup_response(final_content);

    // Send reasoning if present
    if let Some(reasoning) = &final_reasoning {
        if !reasoning.is_empty() {
            let _ = tx.send(ChatEvent::Reasoning {
                content: reasoning.clone(),
            }).await;
        }
    }

    // Send done event
    let _ = tx.send(ChatEvent::Done {
        content: cleaned_content.clone(),
    }).await;

    // Store and spawn background tasks
    if !cleaned_content.is_empty() {
        if let Err(e) = state.db.store_chat_message(
            "assistant",
            &cleaned_content,
            final_reasoning.as_deref(),
        ) {
            warn!("Failed to store assistant message: {}", e);
        }

        spawn_fact_extraction(state.clone(), user_message, cleaned_content);
        maybe_spawn_summarization(state);
    }
}
