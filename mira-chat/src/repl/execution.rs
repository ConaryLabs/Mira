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

    // Model routing based on task complexity
    let effort = classify(input);
    let model = effort.model();
    let effort_str = effort.effort_for_model();
    println!("  {}", colors::reasoning(&format!("[{} @ {}]", effort_str, model)));

    // Tool continuations: use gpt-5.2 with no reasoning for quality
    const CONTINUATION_MODEL: &str = "gpt-5.2";
    const CONTINUATION_EFFORT: &str = "none";

    // Assemble system prompt.
    // CHEAP MODE: until token usage is under control, we do NOT inject the
    // full assembled context blob (summaries/semantic/code index/recent msgs).
    // We rely on server-side continuity via previous_response_id.
    // Keep only persona + guidelines + small Mira context.
    let base_prompt = build_system_prompt(config.context);

    // Check for handoff context (after a smooth reset)
    let handoff = if let Some(session) = config.session {
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
        println!("  {}", colors::status("[context refreshed]"));
        format!("{}\n\n{}", base_prompt, handoff_blob)
    } else {
        base_prompt
    };

    let tools = get_tools();

    // Track total usage
    let mut total_usage = Usage {
        input_tokens: 0,
        output_tokens: 0,
        input_tokens_details: None,
        output_tokens_details: None,
    };

    // Get previous response ID for continuity (persists across turns)
    // Note: if handoff was consumed, response_id should be None (we're starting fresh)
    let mut current_response_id: Option<String> = if let Some(session) = config.session {
        session.get_response_id().await.unwrap_or(None)
    } else {
        None
    };

    // Initial streaming request
    let mut rx = match config
        .client
        .create_stream(
            input,
            &system_prompt,
            current_response_id.as_deref(),
            effort_str,
            model,
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

        // Continue with tool results - codex-mini for speed, no reasoning
        rx = match config
            .client
            .continue_with_tool_results_stream(
                &prev_id,
                tool_results,
                &system_prompt,
                CONTINUATION_EFFORT,
                CONTINUATION_MODEL,
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

    // SMOOTH RESET: Smart chain reset based on tokens AND cache efficiency
    // Only reset if: tokens > threshold AND cache% < minimum
    // This prevents resetting when cache is working well (saving money)
    use mira_core::{CHAIN_RESET_TOKEN_THRESHOLD, CHAIN_RESET_MIN_CACHE_PCT};

    let cached = total_usage.cached_tokens();
    let cache_pct = if total_usage.input_tokens > 0 {
        (cached as u64 * 100 / total_usage.input_tokens as u64) as u32
    } else {
        100 // No input = effectively 100% cached
    };

    let should_reset = total_usage.input_tokens > CHAIN_RESET_TOKEN_THRESHOLD
        && cache_pct < CHAIN_RESET_MIN_CACHE_PCT;

    if should_reset {
        if let Some(session) = config.session {
            // Don't announce loudly - just log it. User will see "[context refreshed]" next turn.
            tracing::info!(
                "Preparing smooth reset: {}k tokens with {}% cache (threshold: {}k, min cache: {}%)",
                total_usage.input_tokens / 1000,
                cache_pct,
                CHAIN_RESET_TOKEN_THRESHOLD / 1000,
                CHAIN_RESET_MIN_CACHE_PCT
            );
            if let Err(e) = session.clear_response_id_with_handoff().await {
                tracing::warn!("Failed to prepare handoff: {}", e);
            }
        }
    } else if total_usage.input_tokens > CHAIN_RESET_TOKEN_THRESHOLD {
        // Above threshold but cache is good - log but don't reset
        tracing::debug!(
            "Skipping reset: {}k tokens but {}% cached (above {}% threshold)",
            total_usage.input_tokens / 1000,
            cache_pct,
            CHAIN_RESET_MIN_CACHE_PCT
        );
    }

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
    // Create futures for all tool calls (use execute_rich for diff support)
    let tool_futures: Vec<_> = stream_result
        .function_calls
        .iter()
        .map(|(name, call_id, arguments)| {
            let executor = tools.clone();
            let name = name.clone();
            let call_id = call_id.clone();
            let arguments = arguments.clone();
            async move {
                let result = executor.execute_rich(&name, &arguments).await;
                (name, call_id, result)
            }
        })
        .collect();

    // Execute all in parallel
    let results = futures::future::join_all(tool_futures).await;

    // Process results
    let mut tool_results: Vec<(String, String)> = Vec::new();
    for (name, call_id, result) in results {
        let rich_result = result?;
        let result_len = rich_result.output.len();

        // Truncate for display
        let display_result = if result_len > 200 {
            format!("{}... ({} bytes)", &rich_result.output[..200], result_len)
        } else {
            rich_result.output.clone()
        };
        println!("  {} {}", colors::tool_name(&format!("[{}]", name)), colors::tool_result(display_result.trim()));

        // Display diff if available
        if let Some(ref diff) = rich_result.diff {
            if diff.has_changes() {
                let (added, removed) = diff.stats();
                println!("  {} +{} -{}", colors::file_path(&diff.path), colors::success(&added.to_string()), colors::error(&removed.to_string()));

                // Show unified diff (compact)
                let unified = diff.unified_diff();
                let colored = colors::format_diff(&unified);
                // Indent diff output
                for line in colored.lines().take(30) {
                    println!("    {}", line);
                }
                let total_lines = colored.lines().count();
                if total_lines > 30 {
                    println!("    {} ... ({} more lines)", colors::status(""), total_lines - 30);
                }
            }
        }

        tool_results.push((call_id, rich_result.output));
    }

    Ok(tool_results)
}

/// Post-process: batch summarization and auto-compaction
async fn post_process(
    client: &Client,
    session: &Option<Arc<SessionManager>>,
    response_id: &Option<String>,
    _user_input: &str,
    _assistant_response: &str,
) {
    let Some(session) = session else { return };

    // BATCH SUMMARIZATION: Summarize when messages exit the recent window
    // Recent N messages stay raw for full fidelity, older ones get batched
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
