// crates/mira-server/src/tools/core/experts/agentic.rs
// Shared agentic tool-calling loop for expert consultation

use super::ToolContext;
use super::strategy::ReasoningStrategy;
use crate::llm::{ChatResult, Message, Tool, ToolCall, record_llm_usage};
use async_trait::async_trait;
use std::time::Duration;
use tokio::time::timeout;

/// Configuration for the agentic loop.
pub struct AgenticLoopConfig {
    pub max_turns: usize,
    pub timeout: Duration,
    pub llm_call_timeout: Duration,
    pub usage_role: String,
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

/// Run the agentic tool-calling loop shared by single-expert and council modes.
///
/// Handles:
/// - Stateful provider message slicing
/// - Tool call execution (parallel or sequential via handler)
/// - Usage recording
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

    let result = timeout(config.timeout, async {
        loop {
            iterations += 1;
            if iterations > config.max_turns {
                return Err(format!(
                    "Expert exceeded maximum iterations ({}). Partial analysis may be available.",
                    config.max_turns
                ));
            }

            // For stateful providers, only send new tool result messages after the first call.
            // The previous_response_id preserves context server-side.
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
                    messages.clone()
                };

            let result = timeout(
                config.llm_call_timeout,
                chat_client.chat_stateful(
                    messages_to_send,
                    Some(tools.clone()),
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
            if let Some(ref tool_calls) = result.tool_calls {
                if !tool_calls.is_empty() {
                    // Add assistant message (drop reasoning to avoid unbounded growth)
                    let mut assistant_msg = Message::assistant(result.content.clone(), None);
                    assistant_msg.tool_calls = Some(tool_calls.clone());
                    messages.push(assistant_msg);

                    if handler.parallel_execution() {
                        // Execute tools in parallel
                        let tool_futures = tool_calls.iter().map(|tc| async {
                            let result = handler.handle_tool_call(tc).await;
                            handler.on_tool_executed(tc, &result).await;
                            (tc.id.clone(), result)
                        });

                        let tool_results = futures::future::join_all(tool_futures).await;
                        for (id, result) in tool_results {
                            total_tool_calls += 1;
                            messages.push(Message::tool_result(&id, result));
                        }
                    } else {
                        // Execute tools sequentially
                        for tc in tool_calls {
                            total_tool_calls += 1;
                            let tool_result = handler.handle_tool_call(tc).await;
                            handler.on_tool_executed(tc, &tool_result).await;
                            messages.push(Message::tool_result(&tc.id, tool_result));
                        }
                    }

                    continue;
                }
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
