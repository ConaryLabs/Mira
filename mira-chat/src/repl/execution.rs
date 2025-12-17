//! Agentic execution loop
//!
//! Handles the main conversation loop with streaming responses,
//! tool calls, summarization, and context compaction.

use anyhow::Result;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::context::{build_system_prompt, MiraContext};
use crate::reasoning::classify;
use crate::responses::{Client, Usage};
use crate::session::SessionManager;
use crate::tools::{get_tools, ToolExecutor};

use super::colors;
use super::streaming::{process_stream, StreamResult};

/// Configuration for the execution loop
pub struct ExecutionConfig<'a> {
    pub client: &'a Client,
    pub tools: &'a ToolExecutor,
    pub context: &'a MiraContext,
    pub session: &'a Option<Arc<SessionManager>>,
    pub cancelled: &'a Arc<AtomicBool>,
}

/// Result of executing a conversation turn
pub struct ExecutionResult {
    /// New response ID for continuation
    pub response_id: Option<String>,
    /// Total usage across all iterations
    pub usage: Usage,
}

/// Process user input with streaming responses and tool execution
pub async fn execute(input: &str, config: ExecutionConfig<'_>) -> Result<ExecutionResult> {
    // Save user message to session (for invisible persistence)
    if let Some(session) = config.session {
        if let Err(e) = session.save_message("user", input).await {
            tracing::debug!("Failed to save user message: {}", e);
        }
    }

    // Always gpt-5.2, effort based on task complexity
    const MODEL: &str = "gpt-5.2";
    let effort = classify(input);
    let effort_str = effort.effort_for_model();
    println!("  {}", colors::reasoning(&format!("[{}]", effort_str)));

    // Tool continuations: no reasoning needed
    const CONTINUATION_EFFORT: &str = "low";

    // Assemble context using session manager (or fallback to static context)
    //
    // CACHE OPTIMIZATION: Prompt is structured for maximum LLM cache hits.
    // Order from most stable (first) to least stable (last):
    //   1. Base instructions (static, never changes)
    //   2. Project path (stable within session)
    //   3. Corrections, goals, memories (change occasionally)
    //   4. Compaction blob (changes on compaction)
    //   5. Summaries (changes on summarization)
    //   6. Semantic context (changes per query)
    //
    // This ensures the longest possible prefix match for caching.
    //
    // FRESH CHAIN PER TURN: We no longer use previous_response_id across turns.
    // Each turn starts fresh with injected context (summaries, compaction, etc).
    // Tool loops within a turn still chain via current_response_id.
    // This prevents token explosion from accumulated tool call history.
    let system_prompt = if let Some(session) = config.session {
        match session.assemble_context(input).await {
            Ok(assembled) => {
                let base_prompt = build_system_prompt(&assembled.mira_context);
                let extra_context = assembled.format_for_prompt();
                if extra_context.is_empty() {
                    base_prompt
                } else {
                    format!("{}\n\n{}", base_prompt, extra_context)
                }
            }
            Err(e) => {
                tracing::warn!("Failed to assemble context: {}", e);
                build_system_prompt(config.context)
            }
        }
    } else {
        build_system_prompt(config.context)
    };

    let tools = get_tools();

    // Track total usage
    let mut total_usage = Usage {
        input_tokens: 0,
        output_tokens: 0,
        input_tokens_details: None,
        output_tokens_details: None,
    };

    // Track current response ID (starts fresh each turn, chains only within tool loops)
    let mut current_response_id: Option<String> = None;

    // Initial streaming request
    let mut rx = match config
        .client
        .create_stream(
            input,
            &system_prompt,
            current_response_id.as_deref(),
            effort_str,
            MODEL,
            &tools,
        )
        .await
    {
        Ok(rx) => rx,
        Err(e) => {
            eprintln!("Error: {}", e);
            return Ok(ExecutionResult {
                response_id: current_response_id,
                usage: total_usage,
            });
        }
    };

    // Agentic loop - track assistant response text for saving
    let mut full_response_text = String::new();
    const MAX_ITERATIONS: usize = 25;

    for iteration in 0..MAX_ITERATIONS {
        // Process streaming events
        let (stream_result, was_cancelled, response_text) =
            process_stream(&mut rx, config.cancelled).await?;
        full_response_text.push_str(&response_text);

        // If cancelled, break out of the loop
        if was_cancelled {
            break;
        }

        // Update response ID and accumulate usage
        if let Some(ref resp) = stream_result.final_response {
            current_response_id = Some(resp.id.clone());

            // Save response ID to session for persistence
            if let Some(session) = config.session {
                if let Err(e) = session.set_response_id(&resp.id).await {
                    tracing::debug!("Failed to save response ID: {}", e);
                }
            }

            // Accumulate usage including cache details
            if let Some(ref usage) = resp.usage {
                total_usage.input_tokens += usage.input_tokens;
                total_usage.output_tokens += usage.output_tokens;

                // Accumulate cached tokens
                if let Some(ref details) = usage.input_tokens_details {
                    let current = total_usage
                        .input_tokens_details
                        .get_or_insert_with(Default::default);
                    current.cached_tokens += details.cached_tokens;
                }

                // Accumulate reasoning tokens
                if let Some(ref details) = usage.output_tokens_details {
                    let current = total_usage
                        .output_tokens_details
                        .get_or_insert_with(Default::default);
                    current.reasoning_tokens += details.reasoning_tokens;
                }
            }
        }

        // If no function calls, we're done
        if stream_result.function_calls.is_empty() {
            break;
        }

        // Make sure we have a previous response ID before continuing
        let prev_id = match &current_response_id {
            Some(id) if !id.is_empty() => id.clone(),
            _ => {
                eprintln!("  {}", colors::error("[error: no response ID for continuation]"));
                break;
            }
        };

        // Execute function calls in parallel for efficiency
        let num_calls = stream_result.function_calls.len();
        if num_calls > 1 {
            println!("  {}", colors::status(&format!("[executing {} tools in parallel]", num_calls)));
        }

        // Check for cancellation before starting
        if config.cancelled.load(Ordering::SeqCst) {
            println!("  {}", colors::warning("[cancelled]"));
            break;
        }

        // Execute tools in parallel
        let tool_results = execute_tools(&stream_result, config.tools, config.cancelled).await?;

        // Check for cancellation after tool execution
        if config.cancelled.load(Ordering::SeqCst) {
            println!("  {}", colors::warning("[cancelled]"));
            break;
        }

        // Check iteration limit - but still send tool results to keep conversation consistent
        let at_limit = iteration >= MAX_ITERATIONS - 1;
        if at_limit {
            eprintln!("  {}", colors::warning("[max iterations reached, finalizing...]"));
        }

        // Continue with tool results - same model, low reasoning
        rx = match config
            .client
            .continue_with_tool_results_stream(
                &prev_id,
                tool_results,
                &system_prompt,
                CONTINUATION_EFFORT,
                MODEL,
                &tools,
            )
            .await
        {
            Ok(rx) => rx,
            Err(e) => {
                eprintln!("Error continuing: {}", e);
                break;
            }
        };

        // Exit after sending results if we hit the limit
        if at_limit {
            // Drain the final response without executing more tools
            let (_, _, response_text) = process_stream(&mut rx, config.cancelled).await?;
            full_response_text.push_str(&response_text);
            break;
        }
    }

    // Save assistant response to session (for invisible persistence)
    if !full_response_text.is_empty() {
        if let Some(session) = config.session {
            if let Err(e) = session.save_message("assistant", &full_response_text).await {
                tracing::debug!("Failed to save assistant message: {}", e);
            }
        }
    }

    // Post-processing: per-turn summarization and compaction
    post_process(config.client, config.session, &current_response_id, input, &full_response_text).await;

    // Show total usage stats
    print_usage(&total_usage);

    Ok(ExecutionResult {
        response_id: current_response_id,
        usage: total_usage,
    })
}

/// Execute tools in parallel and collect results
async fn execute_tools(
    stream_result: &StreamResult,
    tools: &ToolExecutor,
    _cancelled: &Arc<AtomicBool>,
) -> Result<Vec<(String, String)>> {
    // Create futures for all tool calls
    let tool_futures: Vec<_> = stream_result
        .function_calls
        .iter()
        .map(|(name, call_id, arguments)| {
            let executor = tools.clone();
            let name = name.clone();
            let call_id = call_id.clone();
            let arguments = arguments.clone();
            async move {
                let result = executor.execute(&name, &arguments).await;
                (name, call_id, result)
            }
        })
        .collect();

    // Execute all in parallel
    let results = futures::future::join_all(tool_futures).await;

    // Process results
    let mut tool_results: Vec<(String, String)> = Vec::new();
    for (name, call_id, result) in results {
        let result = result?;
        let result_len = result.len();

        // Truncate for display
        let display_result = if result_len > 200 {
            format!("{}... ({} bytes)", &result[..200], result_len)
        } else {
            result.clone()
        };
        println!("  {} {}", colors::tool_name(&format!("[{}]", name)), colors::tool_result(display_result.trim()));

        tool_results.push((call_id, result));
    }

    Ok(tool_results)
}

/// Post-process: per-turn summarization and auto-compaction
async fn post_process(
    client: &Client,
    session: &Option<Arc<SessionManager>>,
    response_id: &Option<String>,
    user_input: &str,
    assistant_response: &str,
) {
    let Some(session) = session else { return };

    // PER-TURN SUMMARIZATION: Summarize this turn immediately
    // This keeps context compact since we no longer chain previous_response_id
    if !user_input.is_empty() && !assistant_response.is_empty() {
        // Only summarize if there's meaningful content
        let user_preview = if user_input.len() > 200 { &user_input[..200] } else { user_input };
        let asst_preview = if assistant_response.len() > 500 { &assistant_response[..500] } else { assistant_response };

        // Skip summarization for very short exchanges
        if user_input.len() + assistant_response.len() > 100 {
            let messages = vec![
                ("user".to_string(), user_preview.to_string()),
                ("assistant".to_string(), asst_preview.to_string()),
            ];

            if let Ok(summary) = client.summarize_messages(&messages).await {
                // Store as a turn summary (we don't delete messages yet - let threshold handle that)
                if let Err(e) = session.store_turn_summary(&summary).await {
                    tracing::debug!("Failed to store turn summary: {}", e);
                }
            }
        }
    }

    // Check if message summarization is needed (level 1) - batch cleanup
    if let Ok(Some(messages_to_summarize)) = session.check_summarization_needed().await {
        println!(
            "  {}",
            colors::status(&format!("[summarizing {} old messages...]", messages_to_summarize.len()))
        );

        // Format messages for summarization API
        let formatted: Vec<(String, String)> = messages_to_summarize
            .iter()
            .map(|m| (m.role.clone(), m.content.clone()))
            .collect();

        // Call GPT to summarize
        if let Ok(summary) = client.summarize_messages(&formatted).await {
            // Collect message IDs
            let ids: Vec<String> = messages_to_summarize.iter().map(|m| m.id.clone()).collect();

            // Store summary and delete old messages
            if let Err(e) = session.store_summary(&summary, &ids).await {
                tracing::warn!("Failed to store summary: {}", e);
            } else {
                println!("  {}", colors::success("[compressed to summary]"));
            }
        } else {
            tracing::debug!("Summarization API call failed, will retry later");
        }
    }

    // Check if meta-summarization is needed (level 2: summarize summaries)
    if let Ok(Some(summaries_to_compress)) = session.check_meta_summarization_needed().await {
        println!(
            "  {}",
            colors::status(&format!("[meta-summarizing {} summaries...]", summaries_to_compress.len()))
        );

        // Format summaries for meta-summarization
        let formatted: Vec<(String, String)> = summaries_to_compress
            .iter()
            .map(|(_, summary)| ("summary".to_string(), summary.clone()))
            .collect();

        // Call GPT to create meta-summary
        if let Ok(meta_summary) = client.summarize_messages(&formatted).await {
            let ids: Vec<String> = summaries_to_compress.iter().map(|(id, _)| id.clone()).collect();

            if let Err(e) = session.store_meta_summary(&meta_summary, &ids).await {
                tracing::warn!("Failed to store meta-summary: {}", e);
            } else {
                println!("  {}", colors::success("[compressed to meta-summary]"));
            }
        } else {
            tracing::debug!("Meta-summarization API call failed, will retry later");
        }
    }

    // Auto-compact code context when enough files touched
    const AUTO_COMPACT_THRESHOLD: usize = 10;
    let touched_files = session.get_touched_files();
    if touched_files.len() >= AUTO_COMPACT_THRESHOLD {
        if let Some(resp_id) = response_id {
            println!("  {}", colors::status(&format!("[auto-compacting {} files...]", touched_files.len())));

            let context = format!(
                "Code context for project. Files: {}",
                touched_files
                    .iter()
                    .take(20)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            );

            match client.compact(resp_id, &context).await {
                Ok(response) => {
                    if let Err(e) = session
                        .store_compaction(&response.encrypted_content, &touched_files)
                        .await
                    {
                        tracing::warn!("Failed to store compaction: {}", e);
                    } else {
                        session.clear_touched_files();
                        let saved = response.tokens_saved.unwrap_or(0);
                        println!("  {}", colors::success(&format!("[compacted, {} tokens saved]", saved)));
                    }
                }
                Err(e) => {
                    tracing::debug!("Auto-compaction failed: {}", e);
                }
            }
        }
    }
}

/// Print usage statistics
fn print_usage(usage: &Usage) {
    let cached = usage.cached_tokens();
    let cache_pct = if usage.input_tokens > 0 {
        (cached as f32 / usage.input_tokens as f32) * 100.0
    } else {
        0.0
    };

    let reasoning = usage.reasoning_tokens();
    let msg = if reasoning > 0 {
        format!(
            "[tokens: {} in / {} out ({} reasoning), {:.0}% cached]",
            usage.input_tokens, usage.output_tokens, reasoning, cache_pct
        )
    } else {
        format!(
            "[tokens: {} in / {} out, {:.0}% cached]",
            usage.input_tokens, usage.output_tokens, cache_pct
        )
    };
    println!("  {}", colors::status(&msg));
}
