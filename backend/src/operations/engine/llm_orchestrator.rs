// backend/src/operations/engine/llm_orchestrator.rs
// LLM orchestration for intelligent code generation and tool execution (Gemini 3 Pro)

use anyhow::{Context, Result};
use serde_json::Value;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error, info, warn};

use crate::budget::BudgetTracker;
use crate::cache::LlmCache;
use crate::checkpoint::CheckpointManager;
use crate::hooks::{HookEnv, HookManager, HookTrigger};
use crate::llm::provider::{
    ContextWarning as PricingContextWarning, Gemini3Pricing, Gemini3Provider, Message, ToolCall,
    ToolCallInfo, ToolCallResponse,
};
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

/// LLM orchestrator for intelligent tool execution (Gemini 3 Pro)
///
/// Uses Gemini 3 Pro with variable thinking levels for all operations.
/// Handles tool calling loop with automatic result feedback.
/// Integrates with BudgetTracker to track costs and enforce limits.
/// Integrates with LlmCache to reduce API costs (target 80%+ hit rate).
/// Integrates with HookManager to execute pre/post tool hooks.
/// Tools that modify files and should trigger checkpoint creation
const FILE_MODIFYING_TOOLS: &[&str] = &[
    "write_project_file",
    "write_file",
    "edit_project_file",
    "edit_file",
    "delete_file",
    "move_file",
    "rename_file",
];

pub struct LlmOrchestrator {
    provider: Gemini3Provider,
    tool_router: Option<Arc<ToolRouter>>,
    budget_tracker: Option<Arc<BudgetTracker>>,
    cache: Option<Arc<LlmCache>>,
    hook_manager: Option<Arc<RwLock<HookManager>>>,
    checkpoint_manager: Option<Arc<CheckpointManager>>,
}

impl LlmOrchestrator {
    pub fn new(provider: Gemini3Provider, tool_router: Option<Arc<ToolRouter>>) -> Self {
        Self {
            provider,
            tool_router,
            budget_tracker: None,
            cache: None,
            hook_manager: None,
            checkpoint_manager: None,
        }
    }

    /// Create orchestrator with budget tracking, caching, hooks, and checkpoints
    pub fn with_services(
        provider: Gemini3Provider,
        tool_router: Option<Arc<ToolRouter>>,
        budget_tracker: Option<Arc<BudgetTracker>>,
        cache: Option<Arc<LlmCache>>,
        hook_manager: Option<Arc<RwLock<HookManager>>>,
        checkpoint_manager: Option<Arc<CheckpointManager>>,
    ) -> Self {
        Self {
            provider,
            tool_router,
            budget_tracker,
            cache,
            hook_manager,
            checkpoint_manager,
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
                Gemini3Pricing::calculate_cost_auto(tokens_input, tokens_output)
            };

            tracker
                .record_request(
                    user_id,
                    Some(operation_id),
                    "gemini",
                    "gemini-2.5-flash",
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
        let thinking = self.provider.thinking_level().as_str();

        match cache
            .get(
                &messages_json,
                Some(tools),
                TOOL_SYSTEM_PROMPT,
                model,
                Some(thinking),
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
                            thought_signature: None, // Cached responses don't have signatures
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
        let thinking = self.provider.thinking_level().as_str();

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

        let cost = Gemini3Pricing::calculate_cost_auto(response.tokens_input, response.tokens_output);

        if let Err(e) = cache
            .put(
                &messages_json,
                Some(tools),
                TOOL_SYSTEM_PROMPT,
                model,
                Some(thinking),
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
        // Use default context (no project_id or session_id)
        self.execute_with_context(user_id, operation_id, messages, tools, None, operation_id, event_tx)
            .await
    }

    /// Execute a request with tool calling support and project context
    pub async fn execute_with_context(
        &self,
        user_id: &str,
        operation_id: &str,
        messages: Vec<Message>,
        tools: Vec<Value>,
        project_id: Option<&str>,
        session_id: &str,
        event_tx: &mpsc::Sender<OperationEngineEvent>,
    ) -> Result<String> {
        info!("[ORCHESTRATOR] Executing with LLM");
        self.execute_with_tools(user_id, operation_id, messages, tools, project_id, session_id, event_tx)
            .await
    }

    /// Execute using LLM with full tool calling loop
    async fn execute_with_tools(
        &self,
        user_id: &str,
        operation_id: &str,
        mut messages: Vec<Message>,
        tools: Vec<Value>,
        project_id: Option<&str>,
        session_id: &str,
        event_tx: &mpsc::Sender<OperationEngineEvent>,
    ) -> Result<String> {
        let start_time = Instant::now();
        info!(
            operation_id = %operation_id,
            tool_count = tools.len(),
            message_count = messages.len(),
            "Starting LLM orchestration"
        );

        // Check budget before starting
        self.check_budget(user_id).await?;

        let mut accumulated_text = String::new();
        let max_iterations = 10; // Safety limit
        let mut total_tokens_input: i64 = 0;
        let mut total_tokens_output: i64 = 0;
        let mut total_from_cache = false;
        let mut total_tool_calls = 0;

        for iteration in 1..=max_iterations {
            debug!(
                operation_id = %operation_id,
                iteration = iteration,
                max_iterations = max_iterations,
                "LLM iteration"
            );

            // Try cache first
            let (response, from_cache) = if let Some(cached) =
                self.try_cache_get(&messages, &tools).await
            {
                debug!(operation_id = %operation_id, "Cache hit");
                (cached, true)
            } else {
                debug!(operation_id = %operation_id, "Cache miss, calling LLM API");
                let llm_start = Instant::now();

                // Call LLM with tools
                let resp = match self
                    .provider
                    .call_with_tools(messages.clone(), tools.clone())
                    .await
                {
                    Ok(r) => r,
                    Err(e) => {
                        error!(
                            operation_id = %operation_id,
                            error = %e,
                            "LLM API call failed"
                        );
                        return Err(e).context("Failed to call LLM");
                    }
                };

                let llm_duration = llm_start.elapsed();
                info!(
                    operation_id = %operation_id,
                    duration_ms = llm_duration.as_millis() as u64,
                    tokens_input = resp.tokens_input,
                    tokens_output = resp.tokens_output,
                    "LLM API call completed"
                );

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

            // Emit usage info with pricing tier
            let cost_result =
                Gemini3Pricing::calculate_cost_with_info(response.tokens_input, response.tokens_output);
            let _ = event_tx
                .send(OperationEngineEvent::UsageInfo {
                    operation_id: operation_id.to_string(),
                    tokens_input: response.tokens_input,
                    tokens_output: response.tokens_output,
                    pricing_tier: cost_result.tier.as_str().to_string(),
                    cost_usd: if from_cache { 0.0 } else { cost_result.cost },
                    from_cache,
                })
                .await;

            // Emit context warning if approaching or over threshold
            if cost_result.warning != PricingContextWarning::None {
                if let Some(message) = cost_result.warning.message() {
                    let warning_level = match cost_result.warning {
                        PricingContextWarning::Approaching => "approaching",
                        PricingContextWarning::NearThreshold => "near_threshold",
                        PricingContextWarning::OverThreshold => "over_threshold",
                        PricingContextWarning::None => "none",
                    };
                    let _ = event_tx
                        .send(OperationEngineEvent::ContextWarning {
                            operation_id: operation_id.to_string(),
                            warning_level: warning_level.to_string(),
                            message: message.to_string(),
                            tokens_input: response.tokens_input,
                            threshold: Gemini3Pricing::LARGE_CONTEXT_THRESHOLD,
                        })
                        .await;
                }
            }

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
                debug!(operation_id = %operation_id, "No tool calls, execution complete");
                break;
            }

            let call_count = response.tool_calls.len();
            total_tool_calls += call_count;
            info!(
                operation_id = %operation_id,
                tool_call_count = call_count,
                "Processing tool calls"
            );

            // Add the assistant's message WITH tool_calls to conversation history
            let tool_calls_info: Vec<ToolCallInfo> = response.tool_calls.iter().map(|tc| {
                ToolCallInfo {
                    id: tc.id.clone(),
                    name: tc.name.clone(),
                    arguments: tc.arguments.clone(),
                }
            }).collect();

            let assistant_content = response.content.clone().unwrap_or_default();
            messages.push(Message::assistant_with_tool_calls_and_signature(
                assistant_content,
                tool_calls_info,
                response.thought_signature.clone(),
            ));

            // Execute tools and collect results
            for tool_call in response.tool_calls {
                let tool_start = Instant::now();
                let result = self.execute_tool(operation_id, &tool_call, project_id, session_id, event_tx).await?;
                debug!(
                    operation_id = %operation_id,
                    tool_name = %tool_call.name,
                    duration_ms = tool_start.elapsed().as_millis() as u64,
                    "Tool executed"
                );

                // Add tool result to conversation with tool_call_id and tool_name
                messages.push(Message::tool_result(
                    tool_call.id.clone(),
                    tool_call.name.clone(),
                    serde_json::to_string(&result)?,
                ));
            }

            // Safety check
            if iteration >= max_iterations {
                warn!(
                    operation_id = %operation_id,
                    "Max iterations reached, stopping"
                );
                break;
            }
        }

        // Record total cost for all iterations
        // Note: Budget tracking errors are non-fatal - log warning and continue
        if let Err(e) = self.record_cost(
            user_id,
            operation_id,
            total_tokens_input,
            total_tokens_output,
            total_from_cache,
        )
        .await {
            warn!("Failed to record budget (non-fatal): {}", e);
        }

        let actual_cost = if total_from_cache {
            0.0
        } else {
            Gemini3Pricing::calculate_cost_auto(total_tokens_input, total_tokens_output)
        };

        let total_duration = start_time.elapsed();
        info!(
            operation_id = %operation_id,
            duration_ms = total_duration.as_millis() as u64,
            tokens_input = total_tokens_input,
            tokens_output = total_tokens_output,
            tool_calls = total_tool_calls,
            cost_usd = actual_cost,
            from_cache = total_from_cache,
            "LLM orchestration completed"
        );

        Ok(accumulated_text)
    }

    /// Execute a single tool call with pre/post hooks
    async fn execute_tool(
        &self,
        operation_id: &str,
        tool_call: &ToolCall,
        project_id: Option<&str>,
        session_id: &str,
        event_tx: &mpsc::Sender<OperationEngineEvent>,
    ) -> Result<Value> {
        info!("[ORCHESTRATOR] Executing tool: {}", tool_call.name);

        let tool_args_str = serde_json::to_string(&tool_call.arguments).unwrap_or_default();

        // Execute PreToolUse hooks
        if let Some(hook_manager) = &self.hook_manager {
            let manager = hook_manager.read().await;
            let env = HookEnv::for_tool(&tool_call.name, &tool_args_str);

            let (should_continue, hook_results) = manager
                .execute_hooks(HookTrigger::PreToolUse, Some(&tool_call.name), &env)
                .await;

            // Log hook results
            for result in &hook_results {
                if result.success {
                    debug!(
                        "[HOOK] PreToolUse '{}' succeeded in {}ms",
                        result.hook_name, result.duration_ms
                    );
                } else {
                    warn!(
                        "[HOOK] PreToolUse '{}' failed: {}",
                        result.hook_name, result.stderr
                    );
                }
            }

            // If a blocking hook failed, abort tool execution
            if !should_continue {
                let blocked_by = hook_results
                    .iter()
                    .find(|r| !r.success)
                    .map(|r| r.hook_name.clone())
                    .unwrap_or_else(|| "unknown".to_string());

                let error_result = serde_json::json!({
                    "success": false,
                    "error": format!("Tool execution blocked by hook '{}'", blocked_by),
                    "blocked_by_hook": blocked_by
                });

                // Emit blocked event
                let _ = event_tx.send(OperationEngineEvent::ToolExecuted {
                    operation_id: operation_id.to_string(),
                    tool_name: tool_call.name.clone(),
                    tool_type: "file".to_string(),
                    summary: format!("Blocked by hook '{}'", blocked_by),
                    success: false,
                    details: Some(error_result.clone()),
                }).await;

                return Ok(error_result);
            }
        }

        // Create checkpoint before file-modifying tools
        if FILE_MODIFYING_TOOLS.contains(&tool_call.name.as_str()) {
            if let Some(checkpoint_mgr) = &self.checkpoint_manager {
                // Extract file path from tool arguments
                let file_path = tool_call.arguments
                    .get("path")
                    .or_else(|| tool_call.arguments.get("file_path"))
                    .and_then(|v| v.as_str());

                if let Some(path) = file_path {
                    let description = format!("Before {}", tool_call.name);
                    match checkpoint_mgr
                        .create_checkpoint(
                            session_id,
                            Some(operation_id),
                            Some(&tool_call.name),
                            &[path],
                            Some(&description),
                        )
                        .await
                    {
                        Ok(checkpoint_id) => {
                            debug!(
                                "[CHECKPOINT] Created {} before {} on {}",
                                &checkpoint_id[..8],
                                tool_call.name,
                                path
                            );
                        }
                        Err(e) => {
                            warn!(
                                "[CHECKPOINT] Failed to create checkpoint for {}: {}",
                                path, e
                            );
                        }
                    }
                }
            }
        }

        // Route to tool router if available
        let (result, success, summary) = if let Some(router) = &self.tool_router {
            // Use context-aware routing for tools that need project/session info
            match router.route_tool_call_with_context(&tool_call.name, tool_call.arguments.clone(), project_id, session_id).await {
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

        // Execute PostToolUse hooks
        if let Some(hook_manager) = &self.hook_manager {
            let manager = hook_manager.read().await;
            let result_str = serde_json::to_string(&result).unwrap_or_default();
            let env = HookEnv::with_result(
                HookEnv::for_tool(&tool_call.name, &tool_args_str),
                success,
                &result_str,
            );

            let (_should_continue, hook_results) = manager
                .execute_hooks(HookTrigger::PostToolUse, Some(&tool_call.name), &env)
                .await;

            // Log hook results (post-hooks don't block, just log)
            for hr in &hook_results {
                if hr.success {
                    debug!(
                        "[HOOK] PostToolUse '{}' succeeded in {}ms",
                        hr.hook_name, hr.duration_ms
                    );
                } else {
                    warn!(
                        "[HOOK] PostToolUse '{}' failed: {}",
                        hr.hook_name, hr.stderr
                    );
                }
            }
        }

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
    use crate::llm::provider::ThinkingLevel;

    #[test]
    fn test_orchestrator_creation() {
        let provider = Gemini3Provider::new(
            "test-key".to_string(),
            "gemini-3-pro-preview".to_string(),
            ThinkingLevel::High,
        ).expect("Should create provider");
        let _orchestrator = LlmOrchestrator::new(provider, None);

        // Just verify it compiles and creates
        assert!(true);
    }
}
