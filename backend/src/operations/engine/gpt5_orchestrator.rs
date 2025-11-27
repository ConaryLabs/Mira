// backend/src/operations/engine/gpt5_orchestrator.rs
// GPT 5.1-based orchestration for intelligent code generation and tool execution

use anyhow::{Context, Result};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::budget::BudgetTracker;
use crate::cache::LlmCache;
use crate::llm::provider::{Gpt5Pricing, Gpt5Provider, Message, ToolCall, ToolCallInfo, ToolCallResponse};
use crate::operations::engine::tool_router::ToolRouter;

use super::events::OperationEngineEvent;

/// System prompt for tool-calling operations
const TOOL_SYSTEM_PROMPT: &str = "You are an intelligent coding assistant.";

/// Cached tool response format (serialized for storage)
#[derive(serde::Serialize, serde::Deserialize)]
struct CachedToolResponse {
    content: Option<String>,
    tool_calls: Vec<ToolCall>,
    finish_reason: String,
}

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

    /// Convert messages to JSON values for cache key generation
    fn messages_to_json(&self, messages: &[Message]) -> Vec<Value> {
        messages
            .iter()
            .map(|msg| {
                let mut obj = serde_json::json!({
                    "role": msg.role,
                    "content": msg.content
                });
                if let Some(ref call_id) = msg.tool_call_id {
                    obj["tool_call_id"] = Value::String(call_id.clone());
                }
                if let Some(ref tool_calls) = msg.tool_calls {
                    obj["tool_calls"] = serde_json::to_value(tool_calls).unwrap_or(Value::Null);
                }
                obj
            })
            .collect()
    }

    /// Try to get a cached response for the given messages and tools
    async fn try_cache_get(
        &self,
        messages: &[Message],
        tools: &[Value],
    ) -> Option<ToolCallResponse> {
        let cache = self.cache.as_ref()?;

        let messages_json = self.messages_to_json(messages);
        let model = self.provider.model();
        let reasoning = self.provider.reasoning_effort().as_str();

        match cache
            .get(
                &messages_json,
                Some(tools),
                TOOL_SYSTEM_PROMPT,
                model,
                Some(reasoning),
            )
            .await
        {
            Ok(Some(cached)) => {
                // Parse cached response back into ToolCallResponse
                match serde_json::from_str::<CachedToolResponse>(&cached.response) {
                    Ok(parsed) => {
                        info!(
                            "[CACHE] Hit! Returning cached response (saved ${:.6})",
                            cached.cost_usd
                        );
                        Some(ToolCallResponse {
                            content: parsed.content,
                            tool_calls: parsed.tool_calls,
                            finish_reason: parsed.finish_reason,
                            tokens_input: cached.tokens_input,
                            tokens_output: cached.tokens_output,
                        })
                    }
                    Err(e) => {
                        warn!("[CACHE] Failed to parse cached response: {}", e);
                        None
                    }
                }
            }
            Ok(None) => {
                debug!("[CACHE] Miss");
                None
            }
            Err(e) => {
                warn!("[CACHE] Error checking cache: {}", e);
                None
            }
        }
    }

    /// Store a response in the cache
    async fn cache_put(
        &self,
        messages: &[Message],
        tools: &[Value],
        response: &ToolCallResponse,
    ) {
        let Some(cache) = &self.cache else {
            return;
        };

        let messages_json = self.messages_to_json(messages);
        let model = self.provider.model();
        let reasoning = self.provider.reasoning_effort().as_str();

        // Serialize response for caching
        let cached_response = CachedToolResponse {
            content: response.content.clone(),
            tool_calls: response.tool_calls.clone(),
            finish_reason: response.finish_reason.clone(),
        };

        let response_json = match serde_json::to_string(&cached_response) {
            Ok(json) => json,
            Err(e) => {
                warn!("[CACHE] Failed to serialize response: {}", e);
                return;
            }
        };

        let cost = Gpt5Pricing::calculate_cost(response.tokens_input, response.tokens_output);

        if let Err(e) = cache
            .put(
                &messages_json,
                Some(tools),
                TOOL_SYSTEM_PROMPT,
                model,
                Some(reasoning),
                &response_json,
                response.tokens_input,
                response.tokens_output,
                cost,
                None, // Use default TTL
            )
            .await
        {
            warn!("[CACHE] Failed to store response: {}", e);
        } else {
            debug!("[CACHE] Stored response for future use");
        }
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
        let mut total_from_cache = false;

        for iteration in 1..=max_iterations {
            debug!(
                "[ORCHESTRATOR] GPT 5.1 iteration {}/{}",
                iteration, max_iterations
            );

            // Try cache first
            let (response, from_cache) = if let Some(cached) =
                self.try_cache_get(&messages, &tools).await
            {
                (cached, true)
            } else {
                // Call GPT 5.1 with tools
                let resp = self
                    .provider
                    .call_with_tools(messages.clone(), tools.clone())
                    .await
                    .context("Failed to call GPT 5.1")?;

                // Store in cache for future use
                self.cache_put(&messages, &tools, &resp).await;

                (resp, false)
            };

            // Track if any response came from cache (for budget reporting)
            if from_cache {
                total_from_cache = true;
            }

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
            total_from_cache,
        )
        .await?;

        let actual_cost = if total_from_cache {
            0.0
        } else {
            Gpt5Pricing::calculate_cost(total_tokens_input, total_tokens_output)
        };

        info!(
            "[ORCHESTRATOR] Complete: {} input, {} output tokens, ${:.6} cost{}",
            total_tokens_input,
            total_tokens_output,
            actual_cost,
            if total_from_cache { " (from cache)" } else { "" }
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

        // Route to tool router if available
        let (result, success, summary) = if let Some(router) = &self.tool_router {
            match router.route_tool_call(&tool_call.name, tool_call.arguments.clone()).await {
                Ok(result) => {
                    // Check if result indicates success (some tools return success: false in response)
                    let is_success = result.get("success")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true); // Default to true if no success field
                    (result, is_success, format!("Executed {}", tool_call.name))
                }
                Err(e) => {
                    warn!("[ORCHESTRATOR] Tool execution failed: {}", e);
                    let error_result = serde_json::json!({
                        "success": false,
                        "error": e.to_string()
                    });
                    (error_result, false, format!("Failed: {}", e))
                }
            }
        } else {
            warn!("[ORCHESTRATOR] No tool router available");
            let error_result = serde_json::json!({
                "success": false,
                "error": "Tool router not available"
            });
            (error_result, false, "Tool router not available".to_string())
        };

        // Emit tool execution event AFTER execution with actual result
        let _ = event_tx.send(OperationEngineEvent::ToolExecuted {
            operation_id: operation_id.to_string(),
            tool_name: tool_call.name.clone(),
            tool_type: "file".to_string(),
            summary,
            success,
            details: Some(result.clone()),
        }).await;

        Ok(result)
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
