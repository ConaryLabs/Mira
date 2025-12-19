//! Chat processing logic
//!
//! The main agentic loop that handles conversation flow, tool execution,
//! and message persistence.

use anyhow::Result;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::{
    context::{build_system_prompt, build_deepseek_prompt, format_deepseek_context, MiraContext},
    reasoning::classify,
    responses::{Client as GptClient, StreamEvent},
    session::SessionManager,
    tools::{get_tools, get_tool_definitions, ToolExecutor},
    provider::{DeepSeekProvider, Provider, ChatRequest as ProviderChatRequest, StreamEvent as ProviderStreamEvent},
};

use super::types::{ChatEvent, ChatRequest, MessageBlock, ToolCallResult};
use super::AppState;

/// Process a chat request through the agentic loop
pub async fn process_chat(
    state: AppState,
    request: ChatRequest,
    tx: mpsc::Sender<ChatEvent>,
) -> Result<()> {
    // Check if using DeepSeek provider
    if request.provider.as_deref() == Some("deepseek") {
        return process_deepseek_chat(state, request, tx).await;
    }

    let project_path = PathBuf::from(&request.project_path);

    // Model routing based on task complexity
    let effort = classify(&request.message);
    let model = effort.model();
    let reasoning_effort = request
        .reasoning_effort
        .unwrap_or_else(|| effort.effort_for_model().to_string());

    // Tool continuations: gpt-5.2 with minimal reasoning for speed
    const CONTINUATION_MODEL: &str = "gpt-5.2";
    const CONTINUATION_EFFORT: &str = "low";

    // Save user message
    let user_msg_id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    let user_blocks = vec![MessageBlock::Text {
        content: request.message.clone(),
    }];

    if let Some(db) = &state.db {
        let _ = sqlx::query(
            r#"
            INSERT INTO chat_messages (id, role, blocks, created_at)
            VALUES ($1, 'user', $2, $3)
            "#,
        )
        .bind(&user_msg_id)
        .bind(serde_json::to_string(&user_blocks)?)
        .bind(now)
        .execute(db)
        .await;
    }

    // Create session manager for full context assembly
    let session = if let Some(db) = &state.db {
        match SessionManager::new(db.clone(), state.semantic.clone(), request.project_path.clone()).await {
            Ok(s) => Some(Arc::new(s)),
            Err(e) => {
                tracing::warn!("Failed to create session manager: {}", e);
                None
            }
        }
    } else {
        None
    };

    // Assemble system prompt.
    // CHEAP MODE: until token usage is under control, we do NOT inject the
    // full assembled context blob (summaries/semantic/code index/recent msgs).
    // We rely on server-side continuity via previous_response_id.
    // Keep only persona + guidelines + small Mira context.
    let base_prompt = if let Some(db) = &state.db {
        let context = MiraContext::load(db, &request.project_path)
            .await
            .unwrap_or_default();
        build_system_prompt(&context)
    } else {
        build_system_prompt(&MiraContext::default())
    };

    // Check for handoff context (after a smooth reset)
    let handoff = if let Some(ref session) = session {
        match session.consume_handoff().await {
            Ok(h) => h,
            Err(e) => {
                tracing::warn!("Failed to consume handoff (continuity may be lost): {}", e);
                None
            }
        }
    } else {
        None
    };

    // If we have a handoff, append it to the system prompt for this turn only
    let system_prompt = if let Some(ref handoff_blob) = handoff {
        tracing::info!("Applying handoff context for smooth continuity");
        format!("{}\n\n{}", base_prompt, handoff_blob)
    } else {
        base_prompt
    };

    // Create GPT client
    let client = GptClient::new(state.api_key.clone());
    let tools = get_tools();

    // Create tool executor with session for file tracking
    let mut executor = ToolExecutor::new();
    executor.cwd = project_path.clone();
    if let Some(db) = &state.db {
        executor = executor.with_db(db.clone());
    }
    executor = executor.with_semantic(state.semantic.clone());
    if let Some(ref s) = session {
        executor = executor.with_session(s.clone());
    }

    // Get previous response ID for continuity from session
    // Note: if handoff was consumed, this should be None (starting fresh)
    let previous_response_id = if let Some(ref session) = session {
        session.get_response_id().await.unwrap_or(None)
    } else {
        get_last_response_id(&state.db).await
    };

    // Agentic loop
    let mut response_id: Option<String> = None;
    let mut assistant_blocks: Vec<MessageBlock> = Vec::new();
    let mut accumulated_text = String::new();
    // Accumulate usage across all iterations
    let mut total_input_tokens: u32 = 0;
    let mut total_output_tokens: u32 = 0;
    let mut total_reasoning_tokens: u32 = 0;
    let mut total_cached_tokens: u32 = 0;

    for iteration in 0..10 {
        // Stream the response
        let mut rx = if iteration == 0 {
            client
                .create_stream(
                    &request.message,
                    &system_prompt,
                    previous_response_id.as_deref(),
                    &reasoning_effort,
                    model,
                    &tools,
                )
                .await?
        } else {
            // Continue with tool results - low reasoning for speed
            let tool_results: Vec<(String, String)> = assistant_blocks
                .iter()
                .filter_map(|b| match b {
                    MessageBlock::ToolCall {
                        call_id, result, ..
                    } => result.as_ref().map(|r| (call_id.clone(), r.output.clone())),
                    _ => None,
                })
                .collect();

            client
                .continue_with_tool_results_stream(
                    response_id.as_ref().expect("response_id must be set after first iteration"),
                    tool_results,
                    &system_prompt,
                    CONTINUATION_EFFORT,
                    CONTINUATION_MODEL,
                    &tools,
                )
                .await?
        };

        // Collect function calls from this iteration
        let mut pending_calls: HashMap<String, (String, String)> = HashMap::new(); // call_id -> (name, args)
        let mut has_function_calls = false;

        while let Some(event) = rx.recv().await {
            match event {
                StreamEvent::TextDelta(delta) => {
                    accumulated_text.push_str(&delta);
                    tx.send(ChatEvent::TextDelta { delta }).await?;
                }
                StreamEvent::FunctionCallStart { name, call_id } => {
                    has_function_calls = true;
                    pending_calls.insert(call_id.clone(), (name.clone(), String::new()));
                    tx.send(ChatEvent::ToolCallStart {
                        call_id,
                        name,
                        arguments: json!({}),
                    })
                    .await?;
                }
                StreamEvent::FunctionCallDelta {
                    call_id,
                    arguments_delta,
                } => {
                    if let Some((_, args)) = pending_calls.get_mut(&call_id) {
                        args.push_str(&arguments_delta);
                    }
                }
                StreamEvent::FunctionCallDone {
                    name,
                    call_id,
                    arguments,
                } => {
                    // Execute the tool with rich result (includes diff for file ops)
                    let rich_result = executor.execute_rich(&name, &arguments).await;
                    let (success, output, diff) = match rich_result {
                        Ok(r) => (r.success, r.output, r.diff),
                        Err(e) => (false, e.to_string(), None),
                    };

                    let tool_result = ToolCallResult {
                        success,
                        output: output.clone(),
                        diff: diff.clone(),
                    };

                    // Parse arguments for storage
                    let args_value: Value =
                        serde_json::from_str(&arguments).unwrap_or(json!({}));

                    // Add to blocks
                    assistant_blocks.push(MessageBlock::ToolCall {
                        call_id: call_id.clone(),
                        name: name.clone(),
                        arguments: args_value.clone(),
                        result: Some(tool_result),
                    });

                    // Send result event
                    tx.send(ChatEvent::ToolCallResult {
                        call_id,
                        name,
                        success,
                        output,
                        diff,
                    })
                    .await?;
                }
                StreamEvent::Done(response) => {
                    response_id = Some(response.id.clone());

                    // Accumulate and send usage
                    if let Some(usage) = response.usage {
                        total_input_tokens += usage.input_tokens;
                        total_output_tokens += usage.output_tokens;
                        total_reasoning_tokens += usage.reasoning_tokens();
                        total_cached_tokens += usage.cached_tokens();
                        tx.send(ChatEvent::Usage {
                            input_tokens: usage.input_tokens,
                            output_tokens: usage.output_tokens,
                            reasoning_tokens: usage.reasoning_tokens(),
                            cached_tokens: usage.cached_tokens(),
                        })
                        .await?;
                    }
                }
                StreamEvent::Error(e) => {
                    tx.send(ChatEvent::Error { message: e }).await?;
                    break;
                }
            }
        }

        // If there were no function calls, we're done
        if !has_function_calls {
            break;
        }
    }

    // Add accumulated text as a block
    if !accumulated_text.is_empty() {
        assistant_blocks.insert(
            0,
            MessageBlock::Text {
                content: accumulated_text,
            },
        );
    }

    // Save assistant message and usage
    save_assistant_message(
        &state.db,
        &session,
        &assistant_blocks,
        &response_id,
        &previous_response_id,
        &reasoning_effort,
        model,
        total_input_tokens,
        total_output_tokens,
        total_reasoning_tokens,
        total_cached_tokens,
    ).await;

    // Send chain info for debugging (after all processing is done)
    let _ = tx.send(ChatEvent::Chain {
        response_id: response_id.clone(),
        previous_response_id,
    }).await;

    Ok(())
}

/// Save assistant message and token usage to database
async fn save_assistant_message(
    db: &Option<SqlitePool>,
    session: &Option<Arc<SessionManager>>,
    assistant_blocks: &[MessageBlock],
    response_id: &Option<String>,
    previous_response_id: &Option<String>,
    reasoning_effort: &str,
    model: &str,
    total_input_tokens: u32,
    total_output_tokens: u32,
    total_reasoning_tokens: u32,
    total_cached_tokens: u32,
) {

    let Some(db) = db else { return };

    let assistant_msg_id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    let _ = sqlx::query(
        r#"
        INSERT INTO chat_messages (id, role, blocks, created_at)
        VALUES ($1, 'assistant', $2, $3)
        "#,
    )
    .bind(&assistant_msg_id)
    .bind(serde_json::to_string(&assistant_blocks).unwrap_or_default())
    .bind(now)
    .execute(db)
    .await;

    // Store token usage for this message (with chain and tool info)
    if total_input_tokens > 0 || total_output_tokens > 0 {
        let usage_id = Uuid::new_v4().to_string();

        // Extract tool call info from assistant blocks
        let tool_calls: Vec<&str> = assistant_blocks
            .iter()
            .filter_map(|b| match b {
                MessageBlock::ToolCall { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect();
        let tool_count = tool_calls.len() as i32;
        let tool_names = if tool_calls.is_empty() {
            None
        } else {
            // Dedupe and join
            let mut unique: Vec<&str> = tool_calls.clone();
            unique.sort();
            unique.dedup();
            Some(unique.join(","))
        };

        let _ = sqlx::query(
            r#"
            INSERT INTO chat_usage (id, message_id, input_tokens, output_tokens, reasoning_tokens, cached_tokens, model, reasoning_effort, created_at, response_id, previous_response_id, tool_count, tool_names)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
            "#,
        )
        .bind(&usage_id)
        .bind(&assistant_msg_id)
        .bind(total_input_tokens as i32)
        .bind(total_output_tokens as i32)
        .bind(total_reasoning_tokens as i32)
        .bind(total_cached_tokens as i32)
        .bind(model)
        .bind(reasoning_effort)
        .bind(now)
        .bind(response_id)
        .bind(previous_response_id)
        .bind(tool_count)
        .bind(&tool_names)
        .execute(db)
        .await;
    }

    // Save response ID for next request (prefer session, fallback to direct)
    if let Some(rid) = response_id {
        if let Some(session) = session {
            let _ = session.set_response_id(rid).await;
        } else {
            let _ = sqlx::query(
                r#"
                INSERT OR REPLACE INTO chat_state (key, value)
                VALUES ('last_response_id', $1)
                "#,
            )
            .bind(rid)
            .execute(db)
            .await;
        }
    }

    // SMOOTH RESET: Smart chain reset with hysteresis
    // Uses hard ceiling (quality guard) + soft reset (cost optimization with hysteresis)
    // Only applies to OpenAI - DeepSeek uses its own path without chain state
    use crate::session::ResetDecision;

    let cache_pct = if total_input_tokens > 0 {
        (total_cached_tokens as u64 * 100 / total_input_tokens as u64) as u32
    } else {
        100 // No input = effectively 100% cached
    };

    if let Some(session) = session {
        match session.should_reset(total_input_tokens, cache_pct).await {
            Ok(ResetDecision::HardReset { reason }) => {
                tracing::info!("Hard reset triggered: {}", reason);
                let _ = session.clear_response_id_with_handoff().await;
                let _ = session.record_reset().await;
            }
            Ok(ResetDecision::SoftReset { reason }) => {
                tracing::info!("Soft reset triggered: {}", reason);
                let _ = session.clear_response_id_with_handoff().await;
                let _ = session.record_reset().await;
            }
            Ok(ResetDecision::Cooldown { turns_remaining }) => {
                tracing::debug!(
                    "Reset skipped: cooldown active ({} turns remaining)",
                    turns_remaining
                );
            }
            Ok(ResetDecision::NoReset) => {
                // Normal operation, no logging needed
            }
            Err(e) => {
                tracing::warn!("Failed to evaluate reset decision: {}", e);
            }
        }
    }
}

/// Get last response ID from legacy chat_state table
async fn get_last_response_id(db: &Option<SqlitePool>) -> Option<String> {
    let Some(db) = db else {
        return None;
    };

    sqlx::query_scalar::<_, String>(
        r#"SELECT value FROM chat_state WHERE key = 'last_response_id'"#,
    )
    .fetch_optional(db)
    .await
    .ok()
    .flatten()
}

/// Process a chat request using DeepSeek V3.2
async fn process_deepseek_chat(
    state: AppState,
    request: ChatRequest,
    tx: mpsc::Sender<ChatEvent>,
) -> Result<()> {
    let project_path = PathBuf::from(&request.project_path);

    // Get DeepSeek API key from environment
    let deepseek_key = std::env::var("DEEPSEEK_API_KEY")
        .map_err(|_| anyhow::anyhow!("DEEPSEEK_API_KEY not set"))?;

    // Create DeepSeek provider (Chat model for tool support)
    let provider = DeepSeekProvider::new_chat(deepseek_key);

    // Build system prompt with FULL assembled context (same as GPT-5.2)
    // DeepSeek doesn't have server-side chain state, so we need to include everything
    let system_prompt = if let Some(db) = &state.db {
        // Create session manager for full context assembly
        let session = SessionManager::new(
            db.clone(),
            state.semantic.clone(),
            request.project_path.clone(),
        ).await;

        match session {
            Ok(s) => {
                // Assemble full context (summaries, semantic, code hints, etc.)
                let assembled = s.assemble_context(&request.message).await.unwrap_or_default();

                tracing::debug!(
                    "DeepSeek context: {} recent msgs, {} summaries, {} semantic hits, {} corrections, {} memories",
                    assembled.recent_messages.len(),
                    assembled.summaries.len(),
                    assembled.semantic_context.len(),
                    assembled.mira_context.corrections.len(),
                    assembled.mira_context.memories.len(),
                );

                let base_prompt = build_deepseek_prompt(&assembled.mira_context);
                let context_blob = format_deepseek_context(&assembled);

                if context_blob.is_empty() {
                    base_prompt
                } else {
                    format!("{}\n\n{}", base_prompt, context_blob)
                }
            }
            Err(e) => {
                tracing::warn!("Failed to create session for DeepSeek: {}", e);
                let context = MiraContext::load(db, &request.project_path)
                    .await
                    .unwrap_or_default();
                build_deepseek_prompt(&context)
            }
        }
    } else {
        build_deepseek_prompt(&MiraContext::default())
    };

    // Create tool executor
    let mut executor = ToolExecutor::new();
    executor.cwd = project_path.clone();
    if let Some(db) = &state.db {
        executor = executor.with_db(db.clone());
    }
    executor = executor.with_semantic(state.semantic.clone());

    // Get tool definitions
    let tools = get_tool_definitions();

    // Build provider request
    let provider_request = ProviderChatRequest::new("deepseek-chat", &system_prompt, &request.message)
        .with_tools(tools);

    // Accumulate usage
    let mut total_input_tokens: u32 = 0;
    let mut total_output_tokens: u32 = 0;
    let mut total_reasoning_tokens: u32 = 0;

    // Agentic loop (max 10 iterations)
    let mut current_request = provider_request;
    let mut assistant_blocks: Vec<MessageBlock> = Vec::new();
    let mut accumulated_text = String::new();

    for _iteration in 0..10 {
        // Stream response
        let mut rx = provider.create_stream(current_request.clone()).await?;

        let mut pending_calls: HashMap<String, (String, String)> = HashMap::new();
        let mut has_function_calls = false;

        while let Some(event) = rx.recv().await {
            match event {
                ProviderStreamEvent::TextDelta(delta) => {
                    accumulated_text.push_str(&delta);
                    tx.send(ChatEvent::TextDelta { delta }).await?;
                }
                ProviderStreamEvent::FunctionCallStart { call_id, name } => {
                    has_function_calls = true;
                    pending_calls.insert(call_id.clone(), (name.clone(), String::new()));
                    tx.send(ChatEvent::ToolCallStart {
                        call_id,
                        name,
                        arguments: json!({}),
                    }).await?;
                }
                ProviderStreamEvent::FunctionCallDelta { call_id, arguments_delta } => {
                    if let Some((_, args)) = pending_calls.get_mut(&call_id) {
                        args.push_str(&arguments_delta);
                    }
                }
                ProviderStreamEvent::FunctionCallEnd { call_id } => {
                    if let Some((name, args)) = pending_calls.remove(&call_id) {
                        // Execute the tool
                        let rich_result = executor.execute_rich(&name, &args).await;
                        let (success, output, diff) = match rich_result {
                            Ok(r) => (r.success, r.output, r.diff),
                            Err(e) => (false, e.to_string(), None),
                        };

                        let args_value: serde_json::Value = serde_json::from_str(&args).unwrap_or(json!({}));

                        assistant_blocks.push(MessageBlock::ToolCall {
                            call_id: call_id.clone(),
                            name: name.clone(),
                            arguments: args_value,
                            result: Some(ToolCallResult {
                                success,
                                output: output.clone(),
                                diff: diff.clone(),
                            }),
                        });

                        tx.send(ChatEvent::ToolCallResult {
                            call_id,
                            name,
                            success,
                            output,
                            diff,
                        }).await?;
                    }
                }
                ProviderStreamEvent::Usage(usage) => {
                    total_input_tokens += usage.input_tokens;
                    total_output_tokens += usage.output_tokens;
                    total_reasoning_tokens += usage.reasoning_tokens;
                    tx.send(ChatEvent::Usage {
                        input_tokens: usage.input_tokens,
                        output_tokens: usage.output_tokens,
                        reasoning_tokens: usage.reasoning_tokens,
                        cached_tokens: 0, // DeepSeek doesn't report cached tokens
                    }).await?;
                }
                ProviderStreamEvent::Error(e) => {
                    tx.send(ChatEvent::Error { message: e }).await?;
                    break;
                }
                ProviderStreamEvent::Done => break,
                _ => {} // Ignore other events
            }
        }

        // If no function calls, we're done
        if !has_function_calls {
            break;
        }

        // Build continuation request with tool results
        // Note: DeepSeek uses client-state, so we need to rebuild context
        // For simplicity, we'll just break after first tool execution
        // Full implementation would need to maintain message history
        break;
    }

    // Add accumulated text as first block
    if !accumulated_text.is_empty() {
        assistant_blocks.insert(0, MessageBlock::Text { content: accumulated_text });
    }

    // Send chain info (no chain for DeepSeek)
    let _ = tx.send(ChatEvent::Chain {
        response_id: None,
        previous_response_id: None,
    }).await;

    Ok(())
}
