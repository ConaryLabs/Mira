//! Chat processing logic
//!
//! The main agentic loop that handles conversation flow, tool execution,
//! and message persistence.
//!
//! Studio uses Gemini 3 Flash by default (cheap, fast) and escalates to
//! Pro when heavy tools are needed (goal, task) or chain depth > 3.
//! It manages goals, tasks, and sends instructions to Claude Code.

// TODO: The outer iteration loop (line ~208) always breaks - needs investigation
// whether multi-iteration tool loops should be supported
#![allow(clippy::never_loop)]

use anyhow::Result;
use chrono::Utc;
use serde_json::json;
use std::{collections::HashMap, path::PathBuf};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::chat::{
    conductor::validation::{repair_json, ToolSchemas},
    context::MiraContext,
    context_builder::ContextBuilder,
    session::{Checkpoint, SessionManager},
    tools::ToolExecutor,
    provider::{
        CachedContent, GeminiChatProvider, GeminiModel, Provider,
        ChatRequest as ProviderChatRequest,
        StreamEvent as ProviderStreamEvent,
        Message as ProviderMessage,
        MessageRole,
        ToolContinueRequest,
        ToolResult as ProviderToolResult,
    },
};
use super::{ContextCacheEntry, CACHE_TTL_SECONDS};

use super::markdown_parser::MarkdownStreamParser;
use super::routing::RoutingState;
use super::types::{ChatEvent, ChatRequest, GroundingSourceInfo, MessageBlock, ToolCallResultData};
use super::AppState;
use crate::chat::tools::{tool_category, tool_summary};

/// Process a chat request through the agentic loop (Gemini Flash/Pro Orchestrator)
pub async fn process_chat(
    state: AppState,
    request: ChatRequest,
    tx: mpsc::Sender<ChatEvent>,
) -> Result<()> {
    // Gemini 3 Flash is the default orchestrator model
    // Escalates to Pro when heavy tools are needed
    process_gemini_chat(state, request, tx).await
}

/// Process a chat request using Gemini 3 Flash/Pro (Orchestrator mode)
/// Starts with Flash (cheap) and escalates to Pro when heavy tools are called
async fn process_gemini_chat(
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

    // Create routing state for Flash/Pro model selection
    // Starts with Flash, escalates to Pro when heavy tools are needed
    let mut routing = RoutingState::new();

    // Create Gemini provider (starts with Flash, may escalate to Pro)
    let api_key = std::env::var("GEMINI_API_KEY")
        .map_err(|_| anyhow::anyhow!("GEMINI_API_KEY not set"))?;
    let mut provider = GeminiChatProvider::new(api_key.clone(), routing.model());

    // ========================================================================
    // CONTEXT BUILDING - Using unified ContextBuilder
    // ========================================================================
    // ContextBuilder clearly separates:
    // - CACHED: system prompt + tools (sent to Gemini cache, stable)
    // - FRESH: conversation history + user input (sent every request)
    // ========================================================================

    let mut session_manager: Option<SessionManager> = None;
    let context_builder = if let Some(db) = &state.db {
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

                // Build context using unified ContextBuilder
                let mut builder = ContextBuilder::new(
                    &assembled.mira_context,
                    &assembled,
                    &request.message,
                );

                // Add checkpoint if present
                if let Some(ref cp) = checkpoint {
                    builder = builder.with_checkpoint(cp);
                    tracing::debug!("ContextBuilder: added checkpoint for continuity");
                }

                // Store session manager for checkpoint saving
                session_manager = Some(s);

                builder
            }
            Err(e) => {
                tracing::warn!("Failed to create session for Gemini: {}", e);
                let mira_ctx = MiraContext::load(db, &request.project_path)
                    .await
                    .unwrap_or_default();
                ContextBuilder::minimal(&mira_ctx, &request.message)
            }
        }
    } else {
        ContextBuilder::minimal(&MiraContext::default(), &request.message)
    };

    // Build cached and fresh content with metrics
    let (cached_content, fresh_content, metrics) = context_builder.build();

    // Log context breakdown for debugging
    tracing::info!(
        target: "context",
        cached_tokens = metrics.total_cached,
        fresh_tokens = metrics.total_fresh,
        history_count = fresh_content.messages.len(),
        system_tokens = metrics.system_prompt_tokens,
        mira_tokens = metrics.mira_context_tokens,
        tools_tokens = metrics.tools_tokens,
        "Context breakdown: CACHED={} tokens, FRESH={} tokens ({} messages)",
        metrics.total_cached, metrics.total_fresh, fresh_content.messages.len()
    );

    // Extract what we need
    let system_prompt = cached_content.system_prompt;
    let tools = cached_content.tools;
    let mut conversation_messages = fresh_content.messages;

    // Create tool executor
    let mut executor = ToolExecutor::new();
    executor.cwd = project_path.clone();
    if let Some(db) = &state.db {
        executor = executor.with_db(db.clone());
    }
    executor = executor.with_semantic(state.semantic.clone());

    // Tool validation schemas for auto-repair
    let tool_schemas = ToolSchemas::default();

    // Accumulate usage
    let mut total_input_tokens: u32 = 0;
    let mut total_output_tokens: u32 = 0;
    let mut total_reasoning_tokens: u32 = 0;

    // Agentic loop (max 10 iterations)
    let mut assistant_blocks: Vec<MessageBlock> = Vec::new();
    let mut accumulated_text = String::new();
    // Accumulated reasoning content - must be passed back during tool continuations
    // (DeepSeek Reasoner requires this for continued reasoning, omission triggers 400 errors)
    let mut accumulated_reasoning = String::new();
    // Markdown parser for typed code block events
    let mut markdown_parser = MarkdownStreamParser::new();

    // Correlation tracking for structured frontend events
    let message_id = Uuid::new_v4().to_string();
    let mut seq: u64 = 0;  // Monotonic sequence number for event ordering

    // Track tool execution start times for duration calculation
    let mut tool_start_times: HashMap<String, std::time::Instant> = HashMap::new();

    // ========================================================================
    // GEMINI CONTEXT CACHING - ~75% cost reduction on cached tokens
    // ========================================================================
    // We cache: system_prompt + tools (the CACHED content from ContextBuilder)
    // We send fresh: conversation_messages (the FRESH content)
    // ========================================================================

    let prompt_hash = cached_content.prompt_hash;
    let gemini_cache: Option<CachedContent> = {
        // Check if we have a valid cache for this project
        if let Some(entry) = state.context_caches.get(&request.project_path, prompt_hash).await {
            tracing::info!(
                target: "context",
                project = %request.project_path,
                cached_tokens = entry.cache.token_count,
                "CACHE HIT: Using existing Gemini cache"
            );
            Some(entry.cache)
        } else {
            // Cache miss - create new cache for future requests
            if metrics.cache_invalidated {
                tracing::info!(
                    target: "context",
                    project = %request.project_path,
                    reason = ?metrics.invalidation_reason,
                    "CACHE INVALIDATED: Creating new cache"
                );
            }

            let cache_result = provider.create_cache(&system_prompt, &tools, None, CACHE_TTL_SECONDS).await;
            match cache_result {
                Ok(Some(cache)) => {
                    tracing::info!(
                        target: "context",
                        project = %request.project_path,
                        cached_tokens = cache.token_count,
                        expires = %cache.expire_time,
                        "CACHE CREATED: New Gemini cache with {} tokens",
                        cache.token_count
                    );
                    let entry = ContextCacheEntry {
                        cache: cache.clone(),
                        prompt_hash,
                        created_at: chrono::Utc::now(),
                    };
                    state.context_caches.set(&request.project_path, entry).await;
                    Some(cache)
                }
                Ok(None) => {
                    tracing::debug!(
                        target: "context",
                        project = %request.project_path,
                        estimated_tokens = metrics.total_cached,
                        "CACHE SKIPPED: Content too small for caching"
                    );
                    None
                }
                Err(e) => {
                    tracing::warn!(
                        target: "context",
                        project = %request.project_path,
                        error = %e,
                        "CACHE FAILED: Proceeding without cache"
                    );
                    None
                }
            }
        }
    };

    // Check for project FileSearch store (for RAG grounding)
    let file_search_stores: Vec<String> = if let Some(db) = &state.db {
        sqlx::query_scalar::<_, String>(
            "SELECT fs.store_name FROM file_search_stores fs
             JOIN projects p ON fs.project_id = p.id
             WHERE p.path = ?"
        )
        .bind(&request.project_path)
        .fetch_all(db)
        .await
        .unwrap_or_default()
    } else {
        Vec::new()
    };

    if !file_search_stores.is_empty() {
        tracing::debug!(
            project = %request.project_path,
            stores = ?file_search_stores,
            "Using FileSearch stores for RAG"
        );
    }

    // ========================================================================
    // SEND REQUEST TO GEMINI
    // ========================================================================
    // - If cache exists: send gemini_cache reference + fresh conversation
    // - Otherwise: send full system_prompt + fresh conversation
    // ========================================================================

    let initial_request = ProviderChatRequest::new(routing.model().model_id(), &system_prompt, &request.message)
        .with_messages(conversation_messages.clone())
        .with_tools(tools.clone());

    tracing::debug!(
        target: "context",
        using_cache = gemini_cache.is_some(),
        message_count = conversation_messages.len(),
        "Sending request to Gemini (cache={})",
        if gemini_cache.is_some() { "HIT" } else { "MISS" }
    );

    let mut rx = if let Some(ref cache) = gemini_cache {
        provider.create_stream_with_cache(cache, initial_request).await?
    } else if !file_search_stores.is_empty() {
        // Use FileSearch-enabled stream when stores exist
        provider.create_stream_with_file_search(initial_request, &file_search_stores).await?
    } else {
        provider.create_stream(initial_request).await?
    };

    // Track tool names completed in each iteration for escalation decisions
    let mut iteration_tool_names: Vec<String> = Vec::new();

    for iteration in 0..10 {

        // (name, args, thought_signature)
        let mut pending_calls: HashMap<String, (String, String, Option<String>)> = HashMap::new();
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
                ProviderStreamEvent::ReasoningDelta(delta) => {
                    // Accumulate reasoning for passback during tool continuations
                    accumulated_reasoning.push_str(&delta);
                    // Stream reasoning content to frontend (displayed collapsed)
                    tx.send(ChatEvent::ReasoningDelta { delta }).await?;
                }
                ProviderStreamEvent::FunctionCallStart { call_id, name, thought_signature } => {
                    pending_calls.insert(call_id.clone(), (name.clone(), String::new(), thought_signature.clone()));
                    tool_start_times.insert(call_id.clone(), std::time::Instant::now());
                    seq += 1;
                    tx.send(ChatEvent::ToolCallStart {
                        call_id,
                        name: name.clone(),
                        arguments: json!({}),
                        message_id: message_id.clone(),
                        seq,
                        ts_ms: chrono::Utc::now().timestamp_millis() as u64,
                        summary: tool_summary(&name, &json!({})),
                        category: tool_category(&name),
                        thought_signature,
                    }).await?;
                }
                ProviderStreamEvent::FunctionCallDelta { call_id, arguments_delta } => {
                    if let Some((_, args, _)) = pending_calls.get_mut(&call_id) {
                        args.push_str(&arguments_delta);
                    }
                }
                ProviderStreamEvent::FunctionCallEnd { call_id } => {
                    if let Some((name, args, thought_signature)) = pending_calls.remove(&call_id) {
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

                        // Calculate duration
                        let duration_ms = tool_start_times
                            .remove(&call_id)
                            .map(|start| start.elapsed().as_millis() as u64)
                            .unwrap_or(0);

                        // Truncation handling for large outputs
                        let total_bytes = output.len();
                        let (truncated, output_preview) = if total_bytes > 8192 {
                            (true, format!("{}...", &output[..8192]))
                        } else {
                            (false, output.clone())
                        };

                        assistant_blocks.push(MessageBlock::ToolCall {
                            call_id: call_id.clone(),
                            name: name.clone(),
                            arguments: args_value,
                            summary: tool_summary(&name, &final_args),
                            category: tool_category(&name),
                            result: Some(ToolCallResultData {
                                success,
                                output: output_preview.clone(),
                                duration_ms,
                                truncated,
                                total_bytes,
                                diff: diff.clone(),
                                output_ref: None, // TODO: implement on-demand fetch
                                exit_code: None,  // TODO: extract from bash executor
                                stderr: None,
                            }),
                        });

                        // Track tool result for continuation
                        iteration_tool_results.push(ProviderToolResult {
                            call_id: call_id.clone(),
                            name: name.clone(),
                            output: output.clone(),
                            thought_signature,
                        });

                        // Track tool name for escalation decisions
                        iteration_tool_names.push(name.clone());

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
                            output: output_preview,
                            duration_ms,
                            truncated,
                            total_bytes,
                            diff,
                            output_ref: None,
                            exit_code: None,
                            stderr: None,
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
                        cached_tokens: usage.cached_tokens,
                    }).await?;
                }
                ProviderStreamEvent::GroundingMetadata { search_queries, sources } => {
                    tx.send(ChatEvent::Grounding {
                        search_queries,
                        sources: sources.into_iter().map(|s| GroundingSourceInfo {
                            uri: s.uri,
                            title: s.title,
                        }).collect(),
                    }).await?;
                }
                ProviderStreamEvent::CodeExecution { language, code, output, outcome } => {
                    tx.send(ChatEvent::CodeExecution {
                        language,
                        code,
                        output,
                        outcome,
                    }).await?;
                }
                ProviderStreamEvent::Error(e) => {
                    tx.send(ChatEvent::Error { message: e }).await?;
                    break;
                }
                ProviderStreamEvent::Done => break,
                _ => {} // Ignore other events (e.g., ResponseId for non-chain providers)
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

            // Check for model escalation before continuation
            // Escalate Flash → Pro if heavy tools were called or chain depth exceeded
            let tool_name_refs: Vec<&str> = iteration_tool_names.iter().map(|s| s.as_str()).collect();
            if routing.process_tool_calls(&tool_name_refs) {
                tracing::info!("Model escalation: {} → {}",
                    GeminiModel::Flash.name(),
                    routing.model().name());
                provider = GeminiChatProvider::new(api_key.clone(), routing.model());
            }
            // Clear tool names for next iteration
            iteration_tool_names.clear();

            tracing::debug!("Gemini {} iteration {}: continuing with {} tool results",
                routing.model().name(), iteration, current_tool_results.len());

            // Pass accumulated reasoning content (Gemini uses thinkingConfig, not reasoning passback)
            let reasoning_for_continuation = if accumulated_reasoning.is_empty() {
                None
            } else {
                Some(accumulated_reasoning.clone())
            };

            let continue_request = ToolContinueRequest {
                model: routing.model().model_id().into(),
                system: system_prompt.clone(),
                previous_response_id: None,
                messages: conversation_messages.clone(),
                tool_results: current_tool_results,
                reasoning_effort: None,
                tools: tools.clone(),
                reasoning_content: reasoning_for_continuation,
            };

            // Clear accumulated reasoning after using it
            accumulated_reasoning.clear();

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
                ProviderStreamEvent::ReasoningDelta(delta) => {
                    // Accumulate reasoning for next tool continuation if needed
                    accumulated_reasoning.push_str(&delta);
                    // Stream reasoning content to frontend (displayed collapsed)
                    tx.send(ChatEvent::ReasoningDelta { delta }).await?;
                }
                ProviderStreamEvent::FunctionCallStart { call_id, name, thought_signature } => {
                    pending_calls.insert(call_id.clone(), (name.clone(), String::new(), thought_signature.clone()));
                    tool_start_times.insert(call_id.clone(), std::time::Instant::now());
                    seq += 1;
                    tx.send(ChatEvent::ToolCallStart {
                        call_id,
                        name: name.clone(),
                        arguments: json!({}),
                        message_id: message_id.clone(),
                        seq,
                        ts_ms: chrono::Utc::now().timestamp_millis() as u64,
                        summary: tool_summary(&name, &json!({})),
                        category: tool_category(&name),
                        thought_signature,
                    }).await?;
                }
                ProviderStreamEvent::FunctionCallDelta { call_id, arguments_delta } => {
                    if let Some((_, args, _)) = pending_calls.get_mut(&call_id) {
                        args.push_str(&arguments_delta);
                    }
                }
                ProviderStreamEvent::FunctionCallEnd { call_id } => {
                    if let Some((name, args, thought_signature)) = pending_calls.remove(&call_id) {
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

                        // Calculate duration
                        let duration_ms = tool_start_times
                            .remove(&call_id)
                            .map(|start| start.elapsed().as_millis() as u64)
                            .unwrap_or(0);

                        // Truncation handling for large outputs
                        let total_bytes = output.len();
                        let (truncated, output_preview) = if total_bytes > 8192 {
                            (true, format!("{}...", &output[..8192]))
                        } else {
                            (false, output.clone())
                        };

                        assistant_blocks.push(MessageBlock::ToolCall {
                            call_id: call_id.clone(),
                            name: name.clone(),
                            arguments: args_value,
                            summary: tool_summary(&name, &final_args),
                            category: tool_category(&name),
                            result: Some(ToolCallResultData {
                                success,
                                output: output_preview.clone(),
                                duration_ms,
                                truncated,
                                total_bytes,
                                diff: diff.clone(),
                                output_ref: None,
                                exit_code: None,
                                stderr: None,
                            }),
                        });

                        current_tool_results.push(ProviderToolResult {
                            call_id: call_id.clone(),
                            name: name.clone(),
                            output: output.clone(),
                            thought_signature,
                        });

                        // Track tool name for escalation decisions
                        iteration_tool_names.push(name.clone());

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
                            output: output_preview,
                            duration_ms,
                            truncated,
                            total_bytes,
                            diff,
                            output_ref: None,
                            exit_code: None,
                            stderr: None,
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
                ProviderStreamEvent::GroundingMetadata { search_queries, sources } => {
                    tx.send(ChatEvent::Grounding {
                        search_queries,
                        sources: sources.into_iter().map(|s| GroundingSourceInfo {
                            uri: s.uri,
                            title: s.title,
                        }).collect(),
                    }).await?;
                }
                ProviderStreamEvent::CodeExecution { language, code, output, outcome } => {
                    tx.send(ChatEvent::CodeExecution {
                        language,
                        code,
                        output,
                        outcome,
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

        // Store token usage (with final model used - may have escalated to Pro)
        if total_input_tokens > 0 || total_output_tokens > 0 {
            let usage_id = Uuid::new_v4().to_string();
            let model_name = routing.model().model_id();
            let _ = sqlx::query(
                r#"
                INSERT INTO token_usage (id, message_id, model, input_tokens, output_tokens, reasoning_tokens, cached_tokens, created_at)
                VALUES ($1, $2, $3, $4, $5, $6, 0, $7)
                "#,
            )
            .bind(&usage_id)
            .bind(&assistant_msg_id)
            .bind(model_name)
            .bind(total_input_tokens as i64)
            .bind(total_output_tokens as i64)
            .bind(total_reasoning_tokens as i64)
            .bind(now)
            .execute(db)
            .await;
        }
    }

    // Send chain info (client-state providers don't use response chains)
    let _ = tx.send(ChatEvent::Chain {
        response_id: None,
        previous_response_id: None,
    }).await;

    Ok(())
}
