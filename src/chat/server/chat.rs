//! Chat processing logic
//!
//! The main agentic loop that handles conversation flow, tool execution,
//! and message persistence.

// TODO: The outer iteration loop (line ~208) always breaks - needs investigation
// whether multi-iteration tool loops should be supported
#![allow(clippy::never_loop)]

use anyhow::Result;
use chrono::Utc;
use serde_json::{json, Value};
use std::{collections::HashMap, path::PathBuf};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::chat::{
    conductor::validation::{repair_json, ToolSchemas},
    context::{build_deepseek_prompt, format_deepseek_context, MiraContext},
    session::{Checkpoint, SessionManager},
    tools::{get_tool_definitions, ToolExecutor},
    provider::{
        DeepSeekProvider, Provider,
        ChatRequest as ProviderChatRequest,
        StreamEvent as ProviderStreamEvent,
        Message as ProviderMessage,
        MessageRole,
        ToolContinueRequest,
        ToolResult as ProviderToolResult,
    },
};

use super::markdown_parser::MarkdownStreamParser;
use super::types::{ChatEvent, ChatRequest, MessageBlock, ToolCallResult};
use super::AppState;

/// Process a chat request through the agentic loop (DeepSeek V3.2)
pub async fn process_chat(
    state: AppState,
    request: ChatRequest,
    tx: mpsc::Sender<ChatEvent>,
) -> Result<()> {
    // DeepSeek is the only model - direct processing
    process_deepseek_chat(state, request, tx).await
}

/// Process a chat request using DeepSeek V3.2
async fn process_deepseek_chat(
    state: AppState,
    request: ChatRequest,
    tx: mpsc::Sender<ChatEvent>,
) -> Result<()> {
    let project_path = PathBuf::from(&request.project_path);

    // Acquire per-project lock to prevent race conditions
    // This ensures only one request per project is processed at a time,
    // preventing races in: message counts, summary/archival, chain reset, handoff
    let project_lock = state.project_locks.get_lock(&request.project_path).await;
    let _guard = project_lock.lock().await;
    tracing::debug!(project = %request.project_path, "Acquired project lock");

    // Save user message to database (for frontend display)
    let now = chrono::Utc::now().timestamp();
    if let Some(db) = &state.db {
        let user_msg_id = Uuid::new_v4().to_string();
        let user_blocks = vec![MessageBlock::Text {
            content: request.message.clone(),
        }];
        let _ = sqlx::query(
            r#"
            INSERT INTO chat_messages (id, role, blocks, created_at)
            VALUES ($1, 'user', $2, $3)
            "#,
        )
        .bind(&user_msg_id)
        .bind(serde_json::to_string(&user_blocks).unwrap_or_default())
        .bind(now)
        .execute(db)
        .await;
    }

    // Get DeepSeek API key from environment
    let deepseek_key = std::env::var("DEEPSEEK_API_KEY")
        .map_err(|_| anyhow::anyhow!("DEEPSEEK_API_KEY not set"))?;

    // Create DeepSeek provider (Chat model for tool support)
    let provider = DeepSeekProvider::new_chat(deepseek_key);

    // Build system prompt with FULL assembled context (same as GPT-5.2)
    // DeepSeek doesn't have server-side chain state, so we use checkpoints for continuity
    let mut session_manager: Option<SessionManager> = None;
    let (system_prompt, history_messages, checkpoint) = if let Some(db) = &state.db {
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

                // Load any existing checkpoint for continuity
                let checkpoint = s.load_checkpoint().await.ok().flatten();
                if checkpoint.is_some() {
                    tracing::debug!("DeepSeek: loaded checkpoint for continuity");
                }

                tracing::debug!(
                    "DeepSeek context: {} recent msgs, {} summaries, {} semantic hits, {} corrections, {} memories",
                    assembled.recent_messages.len(),
                    assembled.summaries.len(),
                    assembled.semantic_context.len(),
                    assembled.mira_context.corrections.len(),
                    assembled.mira_context.memories.len(),
                );

                // Convert recent messages to provider format for conversation history
                let history: Vec<ProviderMessage> = assembled.recent_messages.iter().map(|m| {
                    let role = match m.role.as_str() {
                        "user" => MessageRole::User,
                        "assistant" => MessageRole::Assistant,
                        "tool" => MessageRole::Tool,
                        _ => MessageRole::User,
                    };
                    ProviderMessage {
                        role,
                        content: m.content.clone(),
                    }
                }).collect();

                let base_prompt = build_deepseek_prompt(&assembled.mira_context);
                let context_blob = format_deepseek_context(&assembled);

                let prompt = if context_blob.is_empty() {
                    base_prompt
                } else {
                    format!("{}\n\n{}", base_prompt, context_blob)
                };

                // Store session manager for checkpoint saving
                session_manager = Some(s);

                (prompt, history, checkpoint)
            }
            Err(e) => {
                tracing::warn!("Failed to create session for DeepSeek: {}", e);
                let context = MiraContext::load(db, &request.project_path)
                    .await
                    .unwrap_or_default();
                (build_deepseek_prompt(&context), Vec::new(), None)
            }
        }
    } else {
        (build_deepseek_prompt(&MiraContext::default()), Vec::new(), None)
    };

    // Format checkpoint as context if present
    let system_prompt = if let Some(ref cp) = checkpoint {
        format!("{}\n\n# Last Checkpoint\nTask: {}\nLast action: {}\nRemaining: {}\nFiles: {}",
            system_prompt,
            cp.current_task,
            cp.last_action,
            cp.remaining.as_deref().unwrap_or("none"),
            cp.working_files.join(", "))
    } else {
        system_prompt
    };

    // Create tool executor
    let mut executor = ToolExecutor::new()
        .with_web_search(state.web_search_config.clone());
    executor.cwd = project_path.clone();
    if let Some(db) = &state.db {
        executor = executor.with_db(db.clone());
    }
    executor = executor.with_semantic(state.semantic.clone());

    // Get tool definitions
    let tools = get_tool_definitions();

    // Tool validation schemas for auto-repair
    let tool_schemas = ToolSchemas::default();

    // Accumulate usage
    let mut total_input_tokens: u32 = 0;
    let mut total_output_tokens: u32 = 0;
    let mut total_reasoning_tokens: u32 = 0;

    // Track conversation for multi-turn tool use (includes history)
    let mut conversation_messages = history_messages;
    // Add user's current message to conversation
    conversation_messages.push(ProviderMessage {
        role: MessageRole::User,
        content: request.message.clone(),
    });

    // Agentic loop (max 10 iterations)
    let mut assistant_blocks: Vec<MessageBlock> = Vec::new();
    let mut accumulated_text = String::new();
    // Markdown parser for typed code block events
    let mut markdown_parser = MarkdownStreamParser::new();

    // First request
    let initial_request = ProviderChatRequest::new("deepseek-chat", &system_prompt, &request.message)
        .with_messages(conversation_messages.clone())
        .with_tools(tools.clone());
    let mut rx = provider.create_stream(initial_request).await?;

    for iteration in 0..10 {

        let mut pending_calls: HashMap<String, (String, String)> = HashMap::new();
        let mut iteration_tool_results: Vec<ProviderToolResult> = Vec::new();
        let mut iteration_text = String::new();

        while let Some(event) = rx.recv().await {
            match event {
                ProviderStreamEvent::TextDelta(delta) => {
                    accumulated_text.push_str(&delta);
                    iteration_text.push_str(&delta);
                    // Parse through markdown parser for typed code block events
                    for event in markdown_parser.feed(&delta) {
                        tx.send(event).await?;
                    }
                }
                ProviderStreamEvent::FunctionCallStart { call_id, name } => {
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
                        // Parse and validate/repair args before execution
                        let args_value = match repair_json(&args) {
                            Ok(v) => v,
                            Err(_) => json!({}),
                        };

                        // Validate and potentially repair args
                        let validation = tool_schemas.validate(&name, &args_value);
                        let final_args = if let Some(repaired) = validation.repaired_args {
                            tracing::debug!("DeepSeek tool {} args repaired: {:?}",
                                name, validation.issues.iter()
                                    .filter(|i| i.repaired)
                                    .map(|i| &i.message)
                                    .collect::<Vec<_>>());
                            repaired
                        } else {
                            args_value.clone()
                        };

                        // Execute with validated/repaired args
                        let args_str = final_args.to_string();
                        let rich_result = executor.execute_rich(&name, &args_str).await;
                        let (success, output, diff) = match rich_result {
                            Ok(r) => (r.success, r.output, r.diff),
                            Err(e) => (false, e.to_string(), None),
                        };

                        // Check for council response in tool output
                        if let Ok(parsed) = serde_json::from_str::<Value>(&output) {
                            if let Some(council) = parsed.get("council") {
                                let gpt = council.get("gpt-5.2").and_then(|v| v.as_str()).map(String::from);
                                let opus = council.get("opus-4.5").and_then(|v| v.as_str()).map(String::from);
                                let gemini = council.get("gemini-3-pro").and_then(|v| v.as_str()).map(String::from);

                                // Emit council event
                                let _ = tx.send(ChatEvent::Council {
                                    gpt: gpt.clone(),
                                    opus: opus.clone(),
                                    gemini: gemini.clone(),
                                }).await;

                                // Add council block for storage
                                assistant_blocks.push(MessageBlock::Council {
                                    gpt,
                                    opus,
                                    gemini,
                                });
                            }
                        }

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

                        // Track tool result for continuation
                        iteration_tool_results.push(ProviderToolResult {
                            call_id: call_id.clone(),
                            name: name.clone(),
                            output: output.clone(),
                        });

                        // Save checkpoint after successful tool execution
                        if success {
                            if let Some(ref session) = session_manager {
                                // Extract file paths from args if this is a file operation
                                let mut working_files = Vec::new();
                                if let Some(path) = final_args.get("file_path").and_then(|v| v.as_str()) {
                                    working_files.push(path.to_string());
                                }
                                if let Some(path) = final_args.get("path").and_then(|v| v.as_str()) {
                                    working_files.push(path.to_string());
                                }

                                let cp = Checkpoint {
                                    id: Uuid::new_v4().to_string(),
                                    current_task: request.message.chars().take(100).collect(),
                                    last_action: format!("{}: {}", name, output.chars().take(200).collect::<String>()),
                                    remaining: None,
                                    working_files,
                                    artifact_ids: vec![call_id.clone()],
                                    created_at: Utc::now().timestamp(),
                                };
                                if let Err(e) = session.save_checkpoint(&cp).await {
                                    tracing::warn!("Failed to save checkpoint: {}", e);
                                }
                            }
                        }

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

        // If no tool calls this iteration, we're done
        if iteration_tool_results.is_empty() {
            break;
        }

        // Add assistant's response to conversation (for context in next iteration)
        if !iteration_text.is_empty() {
            conversation_messages.push(ProviderMessage {
                role: MessageRole::Assistant,
                content: iteration_text,
            });
        }

        // Add initial tool calls to conversation history for context
        // (The continuation loop will add subsequent tool interactions)
        if !iteration_tool_results.is_empty() {
            let tool_calls_summary: String = iteration_tool_results
                .iter()
                .map(|r| format!("[Called {} tool]", r.name))
                .collect::<Vec<_>>()
                .join(" ");
            conversation_messages.push(ProviderMessage {
                role: MessageRole::Assistant,
                content: tool_calls_summary,
            });

            for result in &iteration_tool_results {
                // DeepSeek has 128K context - allow larger tool outputs
                let truncated = if result.output.len() > 64000 {
                    format!("{}...[truncated]", &result.output[..64000])
                } else {
                    result.output.clone()
                };
                conversation_messages.push(ProviderMessage {
                    role: MessageRole::User,
                    content: format!("[{} result]: {}", result.name, truncated),
                });
            }
        }

        // Continue with tool results in a loop until no more tool calls
        // This handles multi-step chains like: search → fetch → summarize
        let mut current_tool_results = iteration_tool_results;

        loop {
            if current_tool_results.is_empty() {
                break;
            }

            tracing::debug!("DeepSeek iteration {}: continuing with {} tool results", iteration, current_tool_results.len());

            let continue_request = ToolContinueRequest {
                model: "deepseek-chat".into(),
                system: system_prompt.clone(),
                previous_response_id: None,
                messages: conversation_messages.clone(),
                tool_results: current_tool_results,
                reasoning_effort: None,
                tools: tools.clone(),
            };

            rx = provider.continue_with_tools_stream(continue_request).await?;

            // Process the continuation response
            let mut continue_text = String::new();
            current_tool_results = Vec::new(); // Reset for this round

        while let Some(event) = rx.recv().await {
            match event {
                ProviderStreamEvent::TextDelta(delta) => {
                    accumulated_text.push_str(&delta);
                    continue_text.push_str(&delta);
                    // Parse through markdown parser for typed code block events
                    for event in markdown_parser.feed(&delta) {
                        tx.send(event).await?;
                    }
                }
                ProviderStreamEvent::FunctionCallStart { call_id, name } => {
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
                        // Parse and validate/repair args before execution
                        let args_value = match repair_json(&args) {
                            Ok(v) => v,
                            Err(_) => json!({}),
                        };

                        // Validate and potentially repair args
                        let validation = tool_schemas.validate(&name, &args_value);
                        let final_args = if let Some(repaired) = validation.repaired_args {
                            tracing::debug!("DeepSeek continuation tool {} args repaired", name);
                            repaired
                        } else {
                            args_value.clone()
                        };

                        // Execute with validated/repaired args
                        let args_str = final_args.to_string();
                        let rich_result = executor.execute_rich(&name, &args_str).await;
                        let (success, output, diff) = match rich_result {
                            Ok(r) => (r.success, r.output, r.diff),
                            Err(e) => (false, e.to_string(), None),
                        };

                        // Check for council response in tool output
                        if let Ok(parsed) = serde_json::from_str::<Value>(&output) {
                            if let Some(council) = parsed.get("council") {
                                let gpt = council.get("gpt-5.2").and_then(|v| v.as_str()).map(String::from);
                                let opus = council.get("opus-4.5").and_then(|v| v.as_str()).map(String::from);
                                let gemini = council.get("gemini-3-pro").and_then(|v| v.as_str()).map(String::from);

                                // Emit council event
                                let _ = tx.send(ChatEvent::Council {
                                    gpt: gpt.clone(),
                                    opus: opus.clone(),
                                    gemini: gemini.clone(),
                                }).await;

                                // Add council block for storage
                                assistant_blocks.push(MessageBlock::Council {
                                    gpt,
                                    opus,
                                    gemini,
                                });
                            }
                        }

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

                        current_tool_results.push(ProviderToolResult {
                            call_id: call_id.clone(),
                            name: name.clone(),
                            output: output.clone(),
                        });

                        // Save checkpoint after successful continuation tool
                        if success {
                            if let Some(ref session) = session_manager {
                                let mut working_files = Vec::new();
                                if let Some(path) = final_args.get("file_path").and_then(|v| v.as_str()) {
                                    working_files.push(path.to_string());
                                }
                                if let Some(path) = final_args.get("path").and_then(|v| v.as_str()) {
                                    working_files.push(path.to_string());
                                }

                                let cp = Checkpoint {
                                    id: Uuid::new_v4().to_string(),
                                    current_task: request.message.chars().take(100).collect(),
                                    last_action: format!("{}: {}", name, output.chars().take(200).collect::<String>()),
                                    remaining: None,
                                    working_files,
                                    artifact_ids: vec![call_id.clone()],
                                    created_at: Utc::now().timestamp(),
                                };
                                if let Err(e) = session.save_checkpoint(&cp).await {
                                    tracing::warn!("Failed to save continuation checkpoint: {}", e);
                                }
                            }
                        }

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
                        cached_tokens: 0,
                    }).await?;
                }
                ProviderStreamEvent::Error(e) => {
                    tx.send(ChatEvent::Error { message: e }).await?;
                    break;
                }
                ProviderStreamEvent::Done => break,
                _ => {}
            }
        }

            // Update conversation for next continuation round
            // Include both any text AND tool interactions to maintain full history
            if !continue_text.is_empty() {
                conversation_messages.push(ProviderMessage {
                    role: MessageRole::Assistant,
                    content: continue_text,
                });
            }

            // If there were tool calls this round, add them to conversation history
            // This ensures the next continuation has full context of what happened
            if !current_tool_results.is_empty() {
                // Add assistant's tool calls as text (since ProviderMessage doesn't support tool_calls)
                let tool_calls_summary: String = current_tool_results
                    .iter()
                    .map(|r| format!("[Called {} tool]", r.name))
                    .collect::<Vec<_>>()
                    .join(" ");
                conversation_messages.push(ProviderMessage {
                    role: MessageRole::Assistant,
                    content: tool_calls_summary,
                });

                // Add tool results as user messages (OpenAI format puts these as user context)
                for result in &current_tool_results {
                    // DeepSeek has 128K context - allow larger tool outputs
                    let truncated = if result.output.len() > 64000 {
                        format!("{}...[truncated]", &result.output[..64000])
                    } else {
                        result.output.clone()
                    };
                    conversation_messages.push(ProviderMessage {
                        role: MessageRole::User,
                        content: format!("[{} result]: {}", result.name, truncated),
                    });
                }
            }

            // current_tool_results will be checked at top of loop
            // If empty, the loop breaks; if not, we continue with more tool results
        } // end of continuation loop

        // After all continuations complete, break the outer loop
        break;
    }

    // Flush markdown parser for any unclosed code blocks
    for event in markdown_parser.flush() {
        let _ = tx.send(event).await;
    }

    // Add accumulated text as first block
    if !accumulated_text.is_empty() {
        assistant_blocks.insert(0, MessageBlock::Text { content: accumulated_text.clone() });
    }

    // Save assistant message to database (for frontend display)
    if let Some(db) = &state.db {
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

        // Store token usage
        if total_input_tokens > 0 || total_output_tokens > 0 {
            let usage_id = Uuid::new_v4().to_string();
            let _ = sqlx::query(
                r#"
                INSERT INTO token_usage (id, message_id, model, input_tokens, output_tokens, reasoning_tokens, cached_tokens, created_at)
                VALUES ($1, $2, 'deepseek-chat', $3, $4, $5, 0, $6)
                "#,
            )
            .bind(&usage_id)
            .bind(&assistant_msg_id)
            .bind(total_input_tokens as i64)
            .bind(total_output_tokens as i64)
            .bind(total_reasoning_tokens as i64)
            .bind(now)
            .execute(db)
            .await;
        }
    }

    // Send chain info (no chain for DeepSeek)
    let _ = tx.send(ChatEvent::Chain {
        response_id: None,
        previous_response_id: None,
    }).await;

    Ok(())
}
