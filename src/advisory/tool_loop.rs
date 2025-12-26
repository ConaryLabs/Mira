//! Unified Tool Loop - Generic tool calling loop for all advisory providers
//!
//! Consolidates the duplicated loop logic from tool_loops/{gpt,gemini,opus,deepseek}.rs
//! into a single generic implementation with provider-specific traits.

use anyhow::Result;
use async_trait::async_trait;
use std::time::Duration;
use tokio::time::timeout;

use super::providers::{AdvisoryResponse, AdvisoryUsage};
use super::tool_bridge::{self, AllowedTool, ToolContext, ToolResult};

// ============================================================================
// Tool Definition (for API schemas)
// ============================================================================

/// Tool definition passed to providers
#[derive(Debug, Clone)]
pub struct ToolDefinition {
    pub tool: AllowedTool,
}

impl ToolDefinition {
    pub fn all() -> Vec<Self> {
        AllowedTool::all().into_iter().map(|t| Self { tool: t }).collect()
    }
}

// ============================================================================
// ToolLoopProvider Trait
// ============================================================================

/// Trait for providers that support tool calling loops.
///
/// Each provider implements this trait with its specific:
/// - Conversation state format (message history)
/// - Raw response type (for logging/debugging)
/// - API call logic
#[async_trait]
pub trait ToolLoopProvider: Send + Sync {
    /// The conversation history state (e.g., Vec<Message>, custom types)
    type State: Clone + Send;

    /// The provider-specific raw response for logging/debugging
    type RawResponse: Send;

    /// Provider name for logging
    fn name(&self) -> &'static str;

    /// Timeout for individual API calls (not the whole loop)
    fn timeout_secs(&self) -> u64;

    /// Initialize conversation state from the user's prompt
    fn init_conversation(&self, message: &str) -> Self::State;

    /// Make an API call with optional tools.
    ///
    /// Returns:
    /// - AdvisoryResponse: Standardized response (text + tool calls)
    /// - RawResponse: Provider-specific raw response (for signatures, thinking blocks)
    /// - AdvisoryUsage: Token usage for cost tracking
    async fn call(
        &self,
        state: &Self::State,
        system: Option<&str>,
        tools: Option<&[ToolDefinition]>,
    ) -> Result<(AdvisoryResponse, Self::RawResponse, AdvisoryUsage)>;

    /// Append the assistant's output to the conversation state.
    ///
    /// This is separate from add_tool_results because providers differ in how
    /// they store thinking blocks, tool calls, and content in history.
    fn add_assistant_response(
        &self,
        state: &mut Self::State,
        response: &AdvisoryResponse,
        raw: &Self::RawResponse,
    );

    /// Append tool execution results to the conversation state.
    fn add_tool_results(
        &self,
        state: &mut Self::State,
        results: Vec<ToolResult>,
    );
}

// ============================================================================
// Generic Tool Loop
// ============================================================================

/// Configuration for the tool loop
#[derive(Debug, Clone)]
pub struct ToolLoopConfig {
    /// Maximum rounds of tool calling before forcing final response
    pub max_rounds: usize,
    /// Overall timeout for the entire loop (seconds)
    pub loop_timeout_secs: u64,
}

impl Default for ToolLoopConfig {
    fn default() -> Self {
        Self {
            max_rounds: 5,
            loop_timeout_secs: 120, // 2 minutes
        }
    }
}

impl ToolLoopConfig {
    /// Config for DeepSeek Reasoner (slower, needs more time)
    pub fn for_reasoner() -> Self {
        Self {
            max_rounds: 5,
            loop_timeout_secs: 180, // 3 minutes
        }
    }
}

/// Result of running a tool loop
#[derive(Debug)]
pub struct ToolLoopResult {
    pub response: AdvisoryResponse,
    pub total_tool_calls: usize,
    pub rounds_completed: usize,
    pub total_usage: AdvisoryUsage,
}

/// Run the generic tool loop for any provider.
///
/// This function handles:
/// - MAX_TOOL_ROUNDS iteration
/// - Budget tracking and enforcement
/// - Timeout management
/// - Logging
pub async fn run_tool_loop<P: ToolLoopProvider>(
    provider: &P,
    message: &str,
    system: Option<String>,
    ctx: &mut ToolContext,
    config: ToolLoopConfig,
) -> Result<ToolLoopResult> {
    timeout(
        Duration::from_secs(config.loop_timeout_secs),
        run_tool_loop_inner(provider, message, system, ctx, config.max_rounds),
    )
    .await
    .map_err(|_| anyhow::anyhow!(
        "{} tool loop timed out after {} seconds",
        provider.name(),
        config.loop_timeout_secs
    ))?
}

/// Inner implementation of the tool loop (without timeout wrapper)
async fn run_tool_loop_inner<P: ToolLoopProvider>(
    provider: &P,
    message: &str,
    system: Option<String>,
    ctx: &mut ToolContext,
    max_rounds: usize,
) -> Result<ToolLoopResult> {
    let tools = ToolDefinition::all();
    let mut state = provider.init_conversation(message);
    let mut total_tool_calls = 0;
    let mut total_usage = AdvisoryUsage::default();

    tracing::info!(
        "Starting {} tool loop for: {}...",
        provider.name(),
        &message[..message.len().min(50)]
    );

    for round in 0..max_rounds {
        ctx.tracker.new_call();

        let round_start = std::time::Instant::now();
        tracing::info!("{} tool loop round {} starting...", provider.name(), round + 1);

        // Make API call with tools
        let (response, raw, usage) = provider.call(
            &state,
            system.as_deref(),
            Some(&tools),
        ).await?;

        // Accumulate usage
        total_usage = accumulate_usage(total_usage, usage);

        let elapsed = round_start.elapsed();
        tracing::info!(
            "{} round {} API call took {:?}",
            provider.name(),
            round + 1,
            elapsed
        );

        // If no tool calls, we're done
        if response.tool_calls.is_empty() {
            tracing::info!(
                "{} tool loop complete after {} rounds, {} tool calls",
                provider.name(),
                round + 1,
                total_tool_calls
            );
            return Ok(ToolLoopResult {
                response,
                total_tool_calls,
                rounds_completed: round + 1,
                total_usage,
            });
        }

        tracing::info!(
            "Round {}: {} requested {} tool calls: {:?}",
            round + 1,
            provider.name(),
            response.tool_calls.len(),
            response.tool_calls.iter().map(|c| &c.name).collect::<Vec<_>>()
        );

        // Add assistant response to state (before tool results)
        provider.add_assistant_response(&mut state, &response, &raw);

        // Execute tools and collect results
        let mut results = Vec::with_capacity(response.tool_calls.len());
        for call in &response.tool_calls {
            let tool_call = tool_bridge::ToolCall {
                id: call.id.clone(),
                name: call.name.clone(),
                arguments: call.arguments.clone(),
            };
            let result = tool_bridge::execute_tool(ctx, &tool_call).await;
            total_tool_calls += 1;
            results.push(result);
        }

        // Add tool results to state
        provider.add_tool_results(&mut state, results);

        // Check if we've hit budget limits
        if !ctx.tracker.can_call(&ctx.budget) {
            tracing::warn!(
                "Tool budget exhausted after {} calls",
                total_tool_calls
            );
            // Do one more call without tools to get final response
            let (final_response, _, final_usage) = provider.call(
                &state,
                system.as_deref(),
                None, // No tools
            ).await?;
            total_usage = accumulate_usage(total_usage, final_usage);

            return Ok(ToolLoopResult {
                response: final_response,
                total_tool_calls,
                rounds_completed: round + 1,
                total_usage,
            });
        }
    }

    // If we hit max rounds, do a final call without tools
    tracing::warn!(
        "Hit max tool rounds ({}), forcing final response",
        max_rounds
    );
    let (final_response, _, final_usage) = provider.call(
        &state,
        system.as_deref(),
        None, // No tools
    ).await?;
    total_usage = accumulate_usage(total_usage, final_usage);

    Ok(ToolLoopResult {
        response: final_response,
        total_tool_calls,
        rounds_completed: max_rounds,
        total_usage,
    })
}

/// Accumulate usage across multiple API calls
fn accumulate_usage(mut total: AdvisoryUsage, call: AdvisoryUsage) -> AdvisoryUsage {
    total.input_tokens += call.input_tokens;
    total.output_tokens += call.output_tokens;
    total.reasoning_tokens += call.reasoning_tokens;
    total.cache_read_tokens += call.cache_read_tokens;
    total.cache_write_tokens += call.cache_write_tokens;
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_definition_all() {
        let tools = ToolDefinition::all();
        assert!(!tools.is_empty());
        assert!(tools.iter().any(|t| t.tool.name() == "recall"));
    }

    #[test]
    fn test_config_default() {
        let config = ToolLoopConfig::default();
        assert_eq!(config.max_rounds, 5);
        assert_eq!(config.loop_timeout_secs, 120);
    }

    #[test]
    fn test_config_reasoner() {
        let config = ToolLoopConfig::for_reasoner();
        assert_eq!(config.loop_timeout_secs, 180);
    }
}
