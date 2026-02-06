// crates/mira-server/src/tools/core/experts/agentic.rs
// Shared agentic tool-calling loop for expert consultation

use super::ToolContext;
use super::strategy::ReasoningStrategy;
use crate::llm::{ChatResult, Message, Tool, ToolCall, estimate_message_tokens, record_llm_usage, truncate_messages_to_budget};
use crate::utils::truncate_at_boundary;
use async_trait::async_trait;
use std::time::Duration;
use tokio::time::timeout;

/// Configuration for the agentic loop.
pub struct AgenticLoopConfig {
    pub max_turns: usize,
    pub timeout: Duration,
    pub llm_call_timeout: Duration,
    pub usage_role: String,
    /// Maximum characters per tool result before truncation (0 = unlimited)
    pub max_tool_result_chars: usize,
    /// Maximum total tool calls across all iterations (0 = unlimited)
    pub max_total_tool_calls: usize,
    /// Maximum parallel tool calls per iteration (0 = unlimited, uses join_all)
    pub max_parallel_tool_calls: usize,
    /// Token budget for context window (0 = no budget management)
    pub context_budget: u64,
}

/// Result from a completed agentic loop.
pub struct AgenticLoopResult {
    pub result: ChatResult,
    pub total_tool_calls: usize,
    pub iterations: usize,
}

/// Trait for handling tool calls during the agentic loop.
/// Implement this to customize tool execution and side effects.
#[async_trait]
pub trait ToolHandler: Send + Sync {
    /// Execute a single tool call and return the result string.
    async fn handle_tool_call(&self, tool_call: &ToolCall) -> String;

    /// Called after a tool call is executed. Used for side effects like broadcasting.
    async fn on_tool_executed(&self, _tool_call: &ToolCall, _result: &str) {}

    /// Whether to execute tool calls in parallel (true) or sequentially (false).
    fn parallel_execution(&self) -> bool {
        true
    }
}

/// Truncate a tool result to the configured maximum length.
fn truncate_tool_result(result: String, max_chars: usize) -> String {
    if max_chars == 0 || result.len() <= max_chars {
        return result;
    }
    let truncated = truncate_at_boundary(&result, max_chars);
    format!(
        "{}\n\n[GUARDRAIL: tool result truncated from {} to {} chars]",
        truncated,
        result.len(),
        truncated.len()
    )
}

/// Run the agentic tool-calling loop shared by single-expert and council modes.
///
/// Handles:
/// - Stateful provider message slicing
/// - Context budget truncation (before each LLM call)
/// - Tool call execution with bounded concurrency (chunked join_all)
/// - Tool result truncation
/// - Total tool call limits
/// - Usage recording with budget warnings
/// - Decoupled strategy (actor + thinker) synthesis
pub async fn run_agentic_loop<C: ToolContext>(
    ctx: &C,
    strategy: &ReasoningStrategy,
    messages: &mut Vec<Message>,
    tools: Vec<Tool>,
    config: &AgenticLoopConfig,
    handler: &(dyn ToolHandler + '_),
) -> Result<AgenticLoopResult, String> {
    let chat_client = strategy.actor().clone();
    let mut total_tool_calls = 0usize;
    let mut iterations = 0usize;
    let mut previous_response_id: Option<String> = None;
    let mut cumulative_tokens: u64 = 0;

    let result = timeout(config.timeout, async {
        loop {
            iterations += 1;
            if iterations > config.max_turns {
                tracing::warn!(
                    "GUARDRAIL: expert exceeded max iterations ({})",
                    config.max_turns
                );
                return Err(format!(
                    "Expert exceeded maximum iterations ({}). Partial analysis may be available.",
                    config.max_turns
                ));
            }

            // Check total tool call limit — allow one final tool-free LLM call to synthesize
            let budget_exhausted =
                config.max_total_tool_calls > 0 && total_tool_calls >= config.max_total_tool_calls;

            // For stateful providers, only send new tool result messages after the first call.
            let messages_to_send =
                if previous_response_id.is_some() && chat_client.supports_stateful() {
                    messages
                        .iter()
                        .rev()
                        .take_while(|m| m.role == "tool")
                        .cloned()
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect()
                } else {
                    // Apply context budget truncation for non-stateful providers
                    let mut msgs = messages.clone();
                    if config.context_budget > 0 {
                        msgs = truncate_messages_to_budget(msgs, config.context_budget);
                        if msgs.len() != messages.len() {
                            tracing::info!(
                                "GUARDRAIL: truncated messages from {} to {} for budget {}",
                                messages.len(),
                                msgs.len(),
                                config.context_budget
                            );
                        }
                    }
                    msgs
                };

            // When budget is exhausted, send no tools so the model produces a final text response
            let tools_for_call = if budget_exhausted {
                tracing::info!(
                    "GUARDRAIL: tool budget exhausted ({}), requesting final synthesis",
                    config.max_total_tool_calls
                );
                None
            } else {
                Some(tools.clone())
            };

            let result = timeout(
                config.llm_call_timeout,
                chat_client.chat_stateful(
                    messages_to_send,
                    tools_for_call,
                    previous_response_id.as_deref(),
                ),
            )
            .await
            .map_err(|_| {
                format!(
                    "LLM call timed out after {}s",
                    config.llm_call_timeout.as_secs()
                )
            })?
            .map_err(|e| format!("Expert consultation failed: {}", e))?;

            // Track cumulative tokens and warn at 80% of budget
            if let Some(ref usage) = result.usage {
                cumulative_tokens += usage.total_tokens as u64;
                if config.context_budget > 0 {
                    let threshold = config.context_budget * 80 / 100;
                    let estimated = estimate_message_tokens(messages);
                    if estimated > threshold {
                        tracing::warn!(
                            "GUARDRAIL: message context at {}% of budget ({}/{})",
                            estimated * 100 / config.context_budget,
                            estimated,
                            config.context_budget
                        );
                    }
                }
            }

            // Record usage
            record_llm_usage(
                ctx.pool(),
                chat_client.provider_type(),
                &chat_client.model_name(),
                &config.usage_role,
                &result,
                ctx.project_id().await,
                ctx.get_session_id().await,
            )
            .await;

            previous_response_id = Some(result.request_id.clone());

            // Process tool calls if any
            if let Some(ref tool_calls) = result.tool_calls
                && !tool_calls.is_empty()
            {
                // Add assistant message (drop reasoning to avoid unbounded growth)
                let mut assistant_msg = Message::assistant(result.content.clone(), None);
                assistant_msg.tool_calls = Some(tool_calls.clone());
                messages.push(assistant_msg);

                // Compute remaining budget for this iteration
                let remaining_budget = if config.max_total_tool_calls > 0 {
                    config.max_total_tool_calls.saturating_sub(total_tool_calls)
                } else {
                    usize::MAX
                };

                // Cap tool calls to remaining budget
                let capped_calls = &tool_calls[..tool_calls.len().min(remaining_budget)];

                if handler.parallel_execution() {
                    // Execute tools with bounded concurrency via chunked join_all
                    let chunk_size = if config.max_parallel_tool_calls > 0 {
                        config.max_parallel_tool_calls
                    } else {
                        capped_calls.len() // no limit
                    };

                    for chunk in capped_calls.chunks(chunk_size) {
                        let tool_futures = chunk.iter().map(|tc| async {
                            let result = handler.handle_tool_call(tc).await;
                            handler.on_tool_executed(tc, &result).await;
                            (tc.id.clone(), result)
                        });

                        let tool_results = futures::future::join_all(tool_futures).await;
                        for (id, result) in tool_results {
                            total_tool_calls += 1;
                            let result = truncate_tool_result(result, config.max_tool_result_chars);
                            messages.push(Message::tool_result(&id, result));
                        }
                    }
                } else {
                    // Execute tools sequentially
                    for tc in capped_calls {
                        total_tool_calls += 1;
                        let tool_result = handler.handle_tool_call(tc).await;
                        handler.on_tool_executed(tc, &tool_result).await;
                        let tool_result = truncate_tool_result(tool_result, config.max_tool_result_chars);
                        messages.push(Message::tool_result(&tc.id, tool_result));
                    }
                }

                // Return dummy tool results for any skipped calls so the LLM gets a response
                if capped_calls.len() < tool_calls.len() {
                    for tc in &tool_calls[capped_calls.len()..] {
                        messages.push(Message::tool_result(
                            &tc.id,
                            "[GUARDRAIL: tool call skipped — budget exhausted]".to_string(),
                        ));
                    }
                }

                continue;
            }

            // No tool calls — handle decoupled strategy (actor + thinker)
            if strategy.is_decoupled() {
                let thinker = strategy.thinker();
                tracing::debug!(
                    iterations,
                    tool_calls = total_tool_calls,
                    "Tool gathering complete, switching to thinker for synthesis"
                );

                let assistant_msg =
                    Message::assistant(result.content.clone(), result.reasoning_content.clone());
                messages.push(assistant_msg);
                messages.push(Message::user(
                    "Based on the tool results above, provide your final expert analysis. \
                     Synthesize the findings into a clear, actionable response."
                        .to_string(),
                ));

                let final_result = thinker
                    .chat_stateful(messages.clone(), None::<Vec<Tool>>, None::<&str>)
                    .await
                    .map_err(|e| format!("Thinker synthesis failed: {}", e))?;

                // Record thinker usage
                let thinker_role = format!("{}:reasoner", config.usage_role);
                record_llm_usage(
                    ctx.pool(),
                    thinker.provider_type(),
                    &thinker.model_name(),
                    &thinker_role,
                    &final_result,
                    ctx.project_id().await,
                    ctx.get_session_id().await,
                )
                .await;

                return Ok(AgenticLoopResult {
                    result: final_result,
                    total_tool_calls,
                    iterations,
                });
            }

            return Ok(AgenticLoopResult {
                result,
                total_tool_calls,
                iterations,
            });
        }
    })
    .await
    .map_err(|_| {
        format!(
            "Expert consultation timed out after {}s",
            config.timeout.as_secs()
        )
    })??;

    Ok(result)
}
