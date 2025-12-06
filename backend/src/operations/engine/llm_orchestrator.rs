// backend/src/operations/engine/llm_orchestrator.rs
// LLM orchestration for intelligent code generation and tool execution
// Supports any LlmProvider (OpenAI, Gemini, etc.) via the router

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
    FunctionCall, Message, OpenAIModel, OpenAIPricing, ToolCallInfo, ToolResponse,
};
use crate::llm::router::{ModelRouter, ModelTier};
use crate::operations::engine::tool_router::ToolRouter;

use super::events::OperationEngineEvent;

/// System prompt for tool-calling operations
const TOOL_SYSTEM_PROMPT: &str = "You are an intelligent coding assistant.";

/// Cached tool response format (serialized for storage)
#[derive(serde::Serialize, serde::Deserialize)]
struct CachedToolResponse {
    text_output: String,
    function_calls: Vec<FunctionCall>,
    tokens_input: i64,
    tokens_output: i64,
}

/// LLM orchestrator for intelligent tool execution
///
/// Uses ModelRouter for multi-tier routing (Fast/Voice/Thinker).
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

/// Maximum size for tool results before truncation (Claude Code pattern: 30k chars)
const MAX_TOOL_RESULT_CHARS: usize = 30000;

/// Truncate large tool results to prevent context bloat
///
/// Claude Code limits tool results to ~30k chars. This prevents a single
/// file read from consuming too much context in subsequent LLM calls.
fn truncate_tool_result(result: &str) -> String {
    if result.len() <= MAX_TOOL_RESULT_CHARS {
        return result.to_string();
    }

    // Show first 10k and last 10k chars with truncation notice
    let head_size = 10000;
    let tail_size = 10000;
    let omitted = result.len() - head_size - tail_size;

    format!(
        "{}...\n\n[TRUNCATED: {} characters omitted to save context. Use offset/limit for specific sections.]\n\n...{}",
        &result[..head_size],
        omitted,
        &result[result.len() - tail_size..]
    )
}

pub struct LlmOrchestrator {
    /// Model router for multi-tier LLM routing (Fast/Voice/Thinker)
    router: Arc<ModelRouter>,
    /// Tool router for dispatching tool calls to handlers
    tool_router: Option<Arc<ToolRouter>>,
    budget_tracker: Option<Arc<BudgetTracker>>,
    cache: Option<Arc<LlmCache>>,
    hook_manager: Option<Arc<RwLock<HookManager>>>,
    checkpoint_manager: Option<Arc<CheckpointManager>>,
}

impl LlmOrchestrator {
    pub fn new(router: Arc<ModelRouter>, tool_router: Option<Arc<ToolRouter>>) -> Self {
        Self {
            router,
            tool_router,
            budget_tracker: None,
            cache: None,
            hook_manager: None,
            checkpoint_manager: None,
        }
    }

    /// Create orchestrator with budget tracking, caching, hooks, and checkpoints
    pub fn with_services(
        router: Arc<ModelRouter>,
        tool_router: Option<Arc<ToolRouter>>,
        budget_tracker: Option<Arc<BudgetTracker>>,
        cache: Option<Arc<LlmCache>>,
        hook_manager: Option<Arc<RwLock<HookManager>>>,
        checkpoint_manager: Option<Arc<CheckpointManager>>,
    ) -> Self {
        Self {
            router,
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
        tier: ModelTier,
        from_cache: bool,
    ) -> Result<()> {
        if let Some(tracker) = &self.budget_tracker {
            // Get model from tier for cost calculation
            let (model, provider_name, model_name) = match tier {
                ModelTier::Fast => (OpenAIModel::Gpt51Mini, "openai", "gpt-5.1-mini"),
                ModelTier::Voice => (OpenAIModel::Gpt51, "openai", "gpt-5.1"),
                ModelTier::Thinker => (OpenAIModel::Gpt51, "openai", "gpt-5.1"),
            };

            let cost = if from_cache {
                0.0
            } else {
                // Use OpenAI pricing for all tiers
                OpenAIPricing::calculate_cost(model, tokens_input, tokens_output)
            };

            tracker
                .record_request(
                    user_id,
                    Some(operation_id),
                    provider_name,
                    model_name,
                    Some(tier.as_str()),
                    tokens_input,
                    tokens_output,
                    cost,
                    from_cache,
                )
                .await?;

            debug!(
                "Recorded budget: {} input, {} output, ${:.6} cost ({})",
                tokens_input, tokens_output, cost, model_name
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
        tier: ModelTier,
    ) -> Option<ToolResponse> {
        use crate::llm::provider::TokenUsage;

        let cache = self.cache.as_ref()?;

        let messages_json = self.messages_to_json(messages);
        let model = tier.as_str();

        match cache
            .get(
                &messages_json,
                Some(tools),
                TOOL_SYSTEM_PROMPT,
                model,
                None, // No thinking level for OpenAI
            )
            .await
        {
            Ok(Some(cached)) => {
                // Parse cached response back into ToolResponse
                match serde_json::from_str::<CachedToolResponse>(&cached.response) {
                    Ok(parsed) => {
                        info!(
                            "[CACHE] Hit! Returning cached response (saved ${:.6})",
                            cached.cost_usd
                        );
                        Some(ToolResponse {
                            id: "cached".to_string(),
                            text_output: parsed.text_output,
                            function_calls: parsed.function_calls,
                            tokens: TokenUsage {
                                input: cached.tokens_input,
                                output: cached.tokens_output,
                                reasoning: 0,
                                cached: cached.tokens_input,
                            },
                            latency_ms: 0,
                            raw_response: Value::Null,
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
        response: &ToolResponse,
        tier: ModelTier,
    ) {
        let Some(cache) = &self.cache else {
            return;
        };

        let messages_json = self.messages_to_json(messages);

        // Serialize response for caching
        let cached_response = CachedToolResponse {
            text_output: response.text_output.clone(),
            function_calls: response.function_calls.clone(),
            tokens_input: response.tokens.input,
            tokens_output: response.tokens.output,
        };

        let response_json = match serde_json::to_string(&cached_response) {
            Ok(json) => json,
            Err(e) => {
                warn!("[CACHE] Failed to serialize response: {}", e);
                return;
            }
        };

        // Get model for cost calculation
        let model = match tier {
            ModelTier::Fast => OpenAIModel::Gpt51Mini,
            ModelTier::Voice | ModelTier::Thinker => OpenAIModel::Gpt51,
        };
        let cost = OpenAIPricing::calculate_cost(model, response.tokens.input, response.tokens.output);

        if let Err(e) = cache
            .put(
                &messages_json,
                Some(tools),
                TOOL_SYSTEM_PROMPT,
                tier.as_str(), // Model tier as cache key
                None, // No thinking level for OpenAI
                &response_json,
                response.tokens.input,
                response.tokens.output,
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

        // Initial routing tier (user-facing chat goes to Voice)
        let current_tier = ModelTier::Voice;

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
                tier = %current_tier.as_str(),
                "LLM iteration"
            );

            // Emit thinking status before LLM call
            let _ = event_tx
                .send(OperationEngineEvent::Thinking {
                    operation_id: operation_id.to_string(),
                    status: "thinking".to_string(),
                    message: "Thinking...".to_string(),
                    tokens_in: total_tokens_input,
                    tokens_out: total_tokens_output,
                    active_tool: None,
                })
                .await;

            // Try cache first
            let (response, from_cache) = if let Some(cached) =
                self.try_cache_get(&messages, &tools, current_tier).await
            {
                debug!(operation_id = %operation_id, "Cache hit");
                (cached, true)
            } else {
                debug!(operation_id = %operation_id, tier = %current_tier.as_str(), "Cache miss, calling LLM API");
                let llm_start = Instant::now();

                // Get the provider from the router
                let provider = self.router.get_provider(current_tier);

                // Call LLM with tools using the generic trait
                let resp = match provider
                    .chat_with_tools(
                        messages.clone(),
                        TOOL_SYSTEM_PROMPT.to_string(),
                        tools.clone(),
                        None, // No special context needed
                    )
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
                    tokens_input = resp.tokens.input,
                    tokens_output = resp.tokens.output,
                    provider = provider.name(),
                    "LLM API call completed"
                );

                // Store in cache for future use
                self.cache_put(&messages, &tools, &resp, current_tier).await;

                (resp, false)
            };

            // Track if any response came from cache (for budget reporting)
            if from_cache {
                total_from_cache = true;
            }

            // Track token usage
            total_tokens_input += response.tokens.input;
            total_tokens_output += response.tokens.output;

            // Calculate cost using OpenAI pricing
            let model = match current_tier {
                ModelTier::Fast => OpenAIModel::Gpt51Mini,
                ModelTier::Voice | ModelTier::Thinker => OpenAIModel::Gpt51,
            };
            let cost = if from_cache {
                0.0
            } else {
                OpenAIPricing::calculate_cost(model, response.tokens.input, response.tokens.output)
            };

            // Emit usage info
            let _ = event_tx
                .send(OperationEngineEvent::UsageInfo {
                    operation_id: operation_id.to_string(),
                    tokens_input: response.tokens.input,
                    tokens_output: response.tokens.output,
                    pricing_tier: current_tier.as_str().to_string(),
                    cost_usd: cost,
                    from_cache,
                })
                .await;

            // Stream any text content
            if !response.text_output.is_empty() {
                accumulated_text.push_str(&response.text_output);

                let _ = event_tx.send(OperationEngineEvent::Streaming {
                    operation_id: operation_id.to_string(),
                    content: response.text_output.clone(),
                }).await;
            }

            // Check if we have function calls
            if response.function_calls.is_empty() {
                debug!(operation_id = %operation_id, "No function calls, execution complete");
                break;
            }

            let call_count = response.function_calls.len();
            total_tool_calls += call_count;
            info!(
                operation_id = %operation_id,
                tool_call_count = call_count,
                "Processing function calls"
            );

            // Add the assistant's message WITH tool_calls to conversation history
            let tool_calls_info: Vec<ToolCallInfo> = response.function_calls.iter().map(|fc| {
                ToolCallInfo {
                    id: fc.id.clone(),
                    name: fc.name.clone(),
                    arguments: fc.arguments.clone(),
                }
            }).collect();

            messages.push(Message::assistant_with_tool_calls(
                response.text_output.clone(),
                tool_calls_info,
            ));

            // Execute tools and collect results
            for func_call in &response.function_calls {
                // Determine tier for next iteration based on accumulated context
                // After tool calls, we stay with Voice tier for user-facing responses
                // (Router will determine tier at next iteration start if needed)

                // Emit thinking status for tool execution
                let _ = event_tx
                    .send(OperationEngineEvent::Thinking {
                        operation_id: operation_id.to_string(),
                        status: "executing_tool".to_string(),
                        message: format!("Running {}...", func_call.name),
                        tokens_in: total_tokens_input,
                        tokens_out: total_tokens_output,
                        active_tool: Some(func_call.name.clone()),
                    })
                    .await;

                let tool_start = Instant::now();
                let result = self.execute_tool(operation_id, func_call, project_id, session_id, event_tx).await?;
                debug!(
                    operation_id = %operation_id,
                    tool_name = %func_call.name,
                    duration_ms = tool_start.elapsed().as_millis() as u64,
                    "Tool executed"
                );

                // Add tool result to conversation with tool_call_id and tool_name
                // Truncate large results to prevent context bloat (Claude Code pattern)
                let result_str = serde_json::to_string(&result)?;
                let truncated_result = truncate_tool_result(&result_str);
                messages.push(Message::tool_result(
                    func_call.id.clone(),
                    func_call.name.clone(),
                    truncated_result,
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
            current_tier,
            total_from_cache,
        )
        .await {
            warn!("Failed to record budget (non-fatal): {}", e);
        }

        // Calculate final cost using OpenAI pricing
        let final_model = match current_tier {
            ModelTier::Fast => OpenAIModel::Gpt51Mini,
            ModelTier::Voice | ModelTier::Thinker => OpenAIModel::Gpt51,
        };
        let actual_cost = if total_from_cache {
            0.0
        } else {
            OpenAIPricing::calculate_cost(final_model, total_tokens_input, total_tokens_output)
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
            tier = current_tier.as_str(),
            "LLM orchestration completed"
        );

        Ok(accumulated_text)
    }

    /// Execute a single tool call with pre/post hooks
    async fn execute_tool(
        &self,
        operation_id: &str,
        func_call: &FunctionCall,
        project_id: Option<&str>,
        session_id: &str,
        event_tx: &mpsc::Sender<OperationEngineEvent>,
    ) -> Result<Value> {
        info!("[ORCHESTRATOR] Executing tool: {}", func_call.name);

        let tool_args_str = serde_json::to_string(&func_call.arguments).unwrap_or_default();

        // Execute PreToolUse hooks
        if let Some(hook_manager) = &self.hook_manager {
            let manager = hook_manager.read().await;
            let env = HookEnv::for_tool(&func_call.name, &tool_args_str);

            let (should_continue, hook_results) = manager
                .execute_hooks(HookTrigger::PreToolUse, Some(&func_call.name), &env)
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
                    tool_name: func_call.name.clone(),
                    tool_type: "file".to_string(),
                    summary: format!("Blocked by hook '{}'", blocked_by),
                    success: false,
                    details: Some(error_result.clone()),
                }).await;

                return Ok(error_result);
            }
        }

        // Create checkpoint before file-modifying tools
        if FILE_MODIFYING_TOOLS.contains(&func_call.name.as_str()) {
            if let Some(checkpoint_mgr) = &self.checkpoint_manager {
                // Extract file path from tool arguments
                let file_path = func_call.arguments
                    .get("path")
                    .or_else(|| func_call.arguments.get("file_path"))
                    .and_then(|v| v.as_str());

                if let Some(path) = file_path {
                    let description = format!("Before {}", func_call.name);
                    match checkpoint_mgr
                        .create_checkpoint(
                            session_id,
                            Some(operation_id),
                            Some(&func_call.name),
                            &[path],
                            Some(&description),
                        )
                        .await
                    {
                        Ok(checkpoint_id) => {
                            debug!(
                                "[CHECKPOINT] Created {} before {} on {}",
                                &checkpoint_id[..8],
                                func_call.name,
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
            match router.route_tool_call_with_context(&func_call.name, func_call.arguments.clone(), project_id, session_id).await {
                Ok(result) => {
                    // Check if result indicates success (some tools return success: false in response)
                    let is_success = result.get("success")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true); // Default to true if no success field
                    (result, is_success, format!("Executed {}", func_call.name))
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
                HookEnv::for_tool(&func_call.name, &tool_args_str),
                success,
                &result_str,
            );

            let (_should_continue, hook_results) = manager
                .execute_hooks(HookTrigger::PostToolUse, Some(&func_call.name), &env)
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
            tool_name: func_call.name.clone(),
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
    use crate::llm::provider::{LlmProvider, Message, Response, TokenUsage, ToolContext, ToolResponse};
    use crate::llm::router::RouterConfig;
    use async_trait::async_trait;
    use std::any::Any;

    // Mock provider for testing
    struct MockProvider {
        name: &'static str,
    }

    #[async_trait]
    impl LlmProvider for MockProvider {
        fn name(&self) -> &'static str {
            self.name
        }

        fn as_any(&self) -> &dyn Any {
            self
        }

        async fn chat(&self, _messages: Vec<Message>, _system: String) -> anyhow::Result<Response> {
            Ok(Response {
                content: format!("Response from {}", self.name),
                model: self.name.to_string(),
                tokens: TokenUsage {
                    input: 100,
                    output: 50,
                    reasoning: 0,
                    cached: 0,
                },
                latency_ms: 100,
            })
        }

        async fn chat_with_tools(
            &self,
            _messages: Vec<Message>,
            _system: String,
            _tools: Vec<Value>,
            _context: Option<ToolContext>,
        ) -> anyhow::Result<ToolResponse> {
            Ok(ToolResponse {
                id: "test".to_string(),
                text_output: format!("Response from {}", self.name),
                function_calls: vec![],
                tokens: TokenUsage {
                    input: 100,
                    output: 50,
                    reasoning: 0,
                    cached: 0,
                },
                latency_ms: 100,
                raw_response: Value::Null,
            })
        }
    }

    fn mock_router() -> Arc<ModelRouter> {
        let fast = Arc::new(MockProvider { name: "fast-mock" }) as Arc<dyn LlmProvider>;
        let voice = Arc::new(MockProvider { name: "voice-mock" }) as Arc<dyn LlmProvider>;
        let thinker = Arc::new(MockProvider { name: "thinker-mock" }) as Arc<dyn LlmProvider>;

        Arc::new(ModelRouter::new(fast, voice, thinker, RouterConfig::default()))
    }

    #[test]
    fn test_orchestrator_creation() {
        let router = mock_router();
        let _orchestrator = LlmOrchestrator::new(router, None);

        // Just verify it compiles and creates
        assert!(true);
    }
}
