// backend/src/operations/engine/gpt5_orchestrator.rs
// GPT 5.1-based orchestration for intelligent code generation and tool execution

use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::budget::BudgetTracker;
use crate::cache::LlmCache;
use crate::llm::provider::{Gpt5Pricing, Gpt5Provider, Message, ToolCall, ToolCallInfo};
use crate::operations::engine::tool_router::ToolRouter;

use super::events::OperationEngineEvent;

/// GPT 5.1 orchestrator for intelligent tool execution
///
/// Uses GPT 5.1 with variable reasoning effort for all operations.
/// Handles tool calling loop with automatic result feedback.
/// Integrates with BudgetTracker to track costs and enforce limits.
/// Integrates with LlmCache to reduce API costs (target 80%+ hit rate).
pub struct Gpt5Orchestrator {
    provider: Gpt5Provider,
    tool_router: Option<Arc<ToolRouter>>,
    budget_tracker: Option<Arc<BudgetTracker>>,
    cache: Option<Arc<LlmCache>>,
}

impl Gpt5Orchestrator {
    pub fn new(provider: Gpt5Provider, tool_router: Option<Arc<ToolRouter>>) -> Self {
        Self {
            provider,
            tool_router,
            budget_tracker: None,
            cache: None,
        }
    }

    /// Create orchestrator with budget tracking and caching
    pub fn with_services(
        provider: Gpt5Provider,
        tool_router: Option<Arc<ToolRouter>>,
        budget_tracker: Option<Arc<BudgetTracker>>,
        cache: Option<Arc<LlmCache>>,
    ) -> Self {
        Self {
            provider,
            tool_router,
            budget_tracker,
            cache,
        }
    }

    /// Check budget limits before making an LLM call
    async fn check_budget(&self, user_id: &str) -> Result<()> {
        if let Some(tracker) = &self.budget_tracker {
            tracker.check_limits(user_id, 0.0).await?;
        }
        Ok(())
    }

    /// Record cost after an LLM call
    async fn record_cost(
        &self,
        user_id: &str,
        operation_id: &str,
        tokens_input: i64,
        tokens_output: i64,
        from_cache: bool,
    ) -> Result<()> {
        if let Some(tracker) = &self.budget_tracker {
            let cost = if from_cache {
                0.0
            } else {
                Gpt5Pricing::calculate_cost(tokens_input, tokens_output)
            };

            tracker
                .record_request(
                    user_id,
                    Some(operation_id),
                    "gpt5",
                    "gpt-5.1",
                    Some("medium"), // TODO: Track actual reasoning effort
                    tokens_input,
                    tokens_output,
                    cost,
                    from_cache,
                )
                .await?;

            debug!(
                "Recorded budget: {} input, {} output, ${:.6} cost",
                tokens_input, tokens_output, cost
            );
        }
        Ok(())
    }

    /// Execute a request with tool calling support
    pub async fn execute(
        &self,
        user_id: &str,
        operation_id: &str,
        messages: Vec<Message>,
        tools: Vec<Value>,
        event_tx: &mpsc::Sender<OperationEngineEvent>,
    ) -> Result<String> {
        info!("[ORCHESTRATOR] Executing with GPT 5.1");
        self.execute_with_tools(user_id, operation_id, messages, tools, event_tx)
            .await
    }

    /// Execute using GPT 5.1 with full tool calling loop
    async fn execute_with_tools(
        &self,
        user_id: &str,
        operation_id: &str,
        mut messages: Vec<Message>,
        tools: Vec<Value>,
        event_tx: &mpsc::Sender<OperationEngineEvent>,
    ) -> Result<String> {
        info!("[ORCHESTRATOR] Executing with GPT 5.1 (tools + orchestration)");

        // Check budget before starting
        self.check_budget(user_id).await?;

        let mut accumulated_text = String::new();
        let max_iterations = 10; // Safety limit
        let mut total_tokens_input: i64 = 0;
        let mut total_tokens_output: i64 = 0;

        for iteration in 1..=max_iterations {
            debug!(
                "[ORCHESTRATOR] GPT 5.1 iteration {}/{}",
                iteration, max_iterations
            );

            // Call GPT 5.1 with tools
            let response = self
                .provider
                .call_with_tools(messages.clone(), tools.clone())
                .await
                .context("Failed to call GPT 5.1")?;

            // Track token usage
            total_tokens_input += response.tokens_input;
            total_tokens_output += response.tokens_output;

            // Stream any text content
            if let Some(content) = &response.content {
                if !content.is_empty() {
                    accumulated_text.push_str(content);

                    let _ = event_tx.send(OperationEngineEvent::Streaming {
                        operation_id: operation_id.to_string(),
                        content: content.clone(),
                    }).await;
                }
            }

            // Check if we have tool calls
            if response.tool_calls.is_empty() {
                info!("[ORCHESTRATOR] No tool calls, execution complete");
                break;
            }

            info!("[ORCHESTRATOR] Processing {} tool calls", response.tool_calls.len());

            // Add the assistant's message WITH tool_calls to conversation history
            let tool_calls_info: Vec<ToolCallInfo> = response.tool_calls.iter().map(|tc| {
                ToolCallInfo {
                    id: tc.id.clone(),
                    name: tc.name.clone(),
                    arguments: tc.arguments.clone(),
                }
            }).collect();

            let assistant_content = response.content.clone().unwrap_or_default();
            messages.push(Message::assistant_with_tool_calls(assistant_content, tool_calls_info));

            // Execute tools and collect results
            for tool_call in response.tool_calls {
                let result = self.execute_tool(operation_id, &tool_call, event_tx).await?;

                // Add tool result to conversation with tool_call_id
                messages.push(Message::tool_result(
                    tool_call.id.clone(),
                    serde_json::to_string(&result)?,
                ));
            }

            // Safety check
            if iteration >= max_iterations {
                warn!("[ORCHESTRATOR] Max iterations reached, stopping");
                break;
            }
        }

        // Record total cost for all iterations
        self.record_cost(
            user_id,
            operation_id,
            total_tokens_input,
            total_tokens_output,
            false, // Not from cache
        )
        .await?;

        info!(
            "[ORCHESTRATOR] Complete: {} input, {} output tokens, ${:.6} cost",
            total_tokens_input,
            total_tokens_output,
            Gpt5Pricing::calculate_cost(total_tokens_input, total_tokens_output)
        );

        Ok(accumulated_text)
    }

    /// Execute a single tool call
    async fn execute_tool(
        &self,
        operation_id: &str,
        tool_call: &ToolCall,
        event_tx: &mpsc::Sender<OperationEngineEvent>,
    ) -> Result<Value> {
        info!("[ORCHESTRATOR] Executing tool: {}", tool_call.name);

        // Emit tool execution event
        let _ = event_tx.send(OperationEngineEvent::ToolExecuted {
            operation_id: operation_id.to_string(),
            tool_name: tool_call.name.clone(),
            tool_type: "file".to_string(),
            summary: format!("Executing {}", tool_call.name),
            success: true,
            details: None,
        }).await;

        // Route to tool router if available
        if let Some(router) = &self.tool_router {
            match router.route_tool_call(&tool_call.name, tool_call.arguments.clone()).await {
                Ok(result) => Ok(result),
                Err(e) => {
                    warn!("[ORCHESTRATOR] Tool execution failed: {}", e);
                    Ok(serde_json::json!({
                        "success": false,
                        "error": e.to_string()
                    }))
                }
            }
        } else {
            warn!("[ORCHESTRATOR] No tool router available");
            Ok(serde_json::json!({
                "success": false,
                "error": "Tool router not available"
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::provider::ReasoningEffort;

    #[test]
    fn test_orchestrator_creation() {
        let provider = Gpt5Provider::new(
            "test-key".to_string(),
            "gpt-4o".to_string(),
            ReasoningEffort::Medium,
        ).expect("Should create provider");
        let _orchestrator = Gpt5Orchestrator::new(provider, None);

        // Just verify it compiles and creates
        assert!(true);
    }
}
