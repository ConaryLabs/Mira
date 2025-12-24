//! Advisory Module - Unified LLM advisory system
//!
//! Consolidates MCP hotline and chat council into a single abstraction with:
//! - Multiple providers (GPT-5.2, Opus 4.5, Gemini 3 Pro)
//! - DeepSeek Reasoner as synthesizer
//! - Multi-turn sessions with tiered memory
//! - Streaming responses
//! - Agentic tool calling (read-only)

mod providers;
pub mod session;
pub mod streaming;
pub mod synthesis;
pub mod tool_bridge;

// Re-exports for external use (some items only used externally)
#[allow(unused_imports)]
pub use providers::{
    AdvisoryProvider, AdvisoryRequest, AdvisoryResponse, AdvisoryEvent,
    AdvisoryModel, AdvisoryCapabilities, AdvisoryUsage, AdvisoryMessage,
    AdvisoryRole, ToolCallRequest, ResponsesInputItem,
    GptProvider, GeminiProvider, OpusProvider, ReasonerProvider,
    // Gemini types for tool loop
    GeminiContent, GeminiPart, GeminiPartResponse, GeminiFunctionCallResponse,
    GeminiInputItem,
};
#[allow(unused_imports)]
pub use synthesis::{
    CouncilSynthesis, ConsensusPoint, Citation, Disagreement,
    ModelPosition, UniqueInsight, SynthesisConfidence,
};
#[allow(unused_imports)]
pub use streaming::{
    StreamingCouncilResult, CouncilProgress,
    DEFAULT_STREAM_TIMEOUT, REASONER_STREAM_TIMEOUT,
};

use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;

/// Main advisory service - entry point for all advisory functionality
pub struct AdvisoryService {
    providers: HashMap<AdvisoryModel, Arc<dyn AdvisoryProvider>>,
}

impl AdvisoryService {
    /// Create service with API keys from environment
    pub fn from_env() -> Result<Self> {
        let mut providers: HashMap<AdvisoryModel, Arc<dyn AdvisoryProvider>> = HashMap::new();

        // Add GPT provider
        if let Ok(gpt) = GptProvider::from_env() {
            providers.insert(AdvisoryModel::Gpt52, Arc::new(gpt));
        }

        // Add Gemini provider
        if let Ok(gemini) = GeminiProvider::from_env() {
            providers.insert(AdvisoryModel::Gemini3Pro, Arc::new(gemini));
        }

        // Add Opus provider
        if let Ok(opus) = OpusProvider::from_env() {
            providers.insert(AdvisoryModel::Opus45, Arc::new(opus));
        }

        // Add DeepSeek Reasoner as synthesizer
        if let Ok(reasoner) = ReasonerProvider::from_env() {
            providers.insert(AdvisoryModel::DeepSeekReasoner, Arc::new(reasoner));
        }

        if providers.is_empty() {
            anyhow::bail!("No advisory providers configured - check API keys");
        }

        Ok(Self { providers })
    }

    /// Quick single-model query
    pub async fn ask(
        &self,
        model: AdvisoryModel,
        message: &str,
    ) -> Result<AdvisoryResponse> {
        let provider = self.providers.get(&model)
            .ok_or_else(|| anyhow::anyhow!("Provider {:?} not configured", model))?;

        provider.complete(AdvisoryRequest {
            message: message.to_string(),
            system: None,
            history: vec![],
            enable_tools: false,
        }).await
    }

    /// Query with tool calling - executes tools in a loop until final response
    ///
    /// The model can call read-only Mira tools to gather context before responding.
    /// Tool calls are limited by the budget in ToolContext.
    /// Supports GPT-5.2 (Responses API) and Gemini 3 Pro (with thought signatures).
    pub async fn ask_with_tools(
        &self,
        model: AdvisoryModel,
        message: &str,
        system: Option<String>,
        ctx: &mut tool_bridge::ToolContext,
    ) -> Result<AdvisoryResponse> {
        // Check for recursive advisory calls
        if ctx.is_recursive() {
            anyhow::bail!("Recursive advisory calls are not allowed");
        }

        match model {
            AdvisoryModel::Gpt52 => self.ask_with_tools_gpt(message, system, ctx).await,
            AdvisoryModel::Gemini3Pro => self.ask_with_tools_gemini(message, system, ctx).await,
            AdvisoryModel::DeepSeekReasoner => self.ask_with_tools_deepseek(message, system, ctx).await,
            _ => {
                // Fall back to simple ask for other providers
                self.ask(model, message).await
            }
        }
    }

    /// GPT-5.2 tool loop using Responses API
    async fn ask_with_tools_gpt(
        &self,
        message: &str,
        system: Option<String>,
        ctx: &mut tool_bridge::ToolContext,
    ) -> Result<AdvisoryResponse> {
        let gpt = GptProvider::from_env()?;

        const MAX_TOOL_ROUNDS: usize = 5;
        let mut items: Vec<ResponsesInputItem> = vec![];
        let mut total_tool_calls = 0;

        // Start with user message
        items.push(ResponsesInputItem::Message {
            role: "user".to_string(),
            content: message.to_string(),
        });

        for round in 0..MAX_TOOL_ROUNDS {
            ctx.tracker.new_call();

            let response = gpt.complete_with_items(
                items.clone(),
                system.clone(),
                true,
            ).await?;

            // If no tool calls, we're done
            if response.tool_calls.is_empty() {
                return Ok(response);
            }

            tracing::debug!(
                "Round {}: GPT requested {} tool calls",
                round + 1,
                response.tool_calls.len()
            );

            // Add function_call items and execute tools
            for call in &response.tool_calls {
                // Add the function_call item (required by Responses API)
                items.push(ResponsesInputItem::FunctionCall {
                    call_id: call.id.clone(),
                    name: call.name.clone(),
                    arguments: serde_json::to_string(&call.arguments)
                        .unwrap_or_else(|_| "{}".to_string()),
                });

                // Execute the tool
                let tool_call = tool_bridge::ToolCall {
                    id: call.id.clone(),
                    name: call.name.clone(),
                    arguments: call.arguments.clone(),
                };
                let result = tool_bridge::execute_tool(ctx, &tool_call).await;
                total_tool_calls += 1;

                // Add function_call_output item
                items.push(ResponsesInputItem::FunctionCallOutput {
                    call_id: call.id.clone(),
                    output: result.content,
                });
            }

            // Check if we've hit budget limits
            if !ctx.tracker.can_call(&ctx.budget) {
                tracing::warn!("Tool budget exhausted after {} calls", total_tool_calls);
                // Do one more call without tools to get final response
                let final_response = gpt.complete_with_items(
                    items,
                    system,
                    false,
                ).await?;
                return Ok(final_response);
            }
        }

        // If we hit max rounds, do a final call without tools
        tracing::warn!("Hit max tool rounds ({}), forcing final response", MAX_TOOL_ROUNDS);
        let final_response = gpt.complete_with_items(
            items,
            system,
            false,
        ).await?;

        Ok(final_response)
    }

    /// Gemini 3 Pro tool loop with thought signature preservation
    ///
    /// Has an overall timeout of 2 minutes to prevent runaway tool loops.
    async fn ask_with_tools_gemini(
        &self,
        message: &str,
        system: Option<String>,
        ctx: &mut tool_bridge::ToolContext,
    ) -> Result<AdvisoryResponse> {
        use std::time::Duration;
        use tokio::time::timeout;

        // Overall timeout for the entire tool loop (2 minutes)
        const TOOL_LOOP_TIMEOUT_SECS: u64 = 120;

        timeout(
            Duration::from_secs(TOOL_LOOP_TIMEOUT_SECS),
            self.ask_with_tools_gemini_inner(message, system, ctx),
        )
        .await
        .map_err(|_| anyhow::anyhow!("Gemini tool loop timed out after {} seconds", TOOL_LOOP_TIMEOUT_SECS))?
    }

    /// Inner implementation of Gemini tool loop
    async fn ask_with_tools_gemini_inner(
        &self,
        message: &str,
        system: Option<String>,
        ctx: &mut tool_bridge::ToolContext,
    ) -> Result<AdvisoryResponse> {
        use providers::{GeminiTextPart, GeminiFunctionCallPart, GeminiFunctionCall,
            GeminiFunctionResponsePart, GeminiFunctionResponse};

        let gemini = GeminiProvider::from_env()?;

        const MAX_TOOL_ROUNDS: usize = 5;
        let mut contents: Vec<GeminiContent> = vec![];
        let mut total_tool_calls = 0;

        tracing::info!("Starting Gemini tool loop for: {}...", &message[..message.len().min(50)]);

        // Start with user message
        contents.push(GeminiContent {
            role: "user".to_string(),
            parts: vec![GeminiPart::Text(GeminiTextPart { text: message.to_string() })],
        });

        for round in 0..MAX_TOOL_ROUNDS {
            ctx.tracker.new_call();

            let round_start = std::time::Instant::now();
            tracing::info!("Gemini tool loop round {} starting...", round + 1);

            let (response, raw_parts) = gemini.complete_with_contents(
                contents.clone(),
                system.clone(),
                true,
            ).await?;

            let elapsed = round_start.elapsed();
            tracing::info!("Gemini round {} API call took {:?}", round + 1, elapsed);

            // If no tool calls, we're done
            if response.tool_calls.is_empty() {
                tracing::info!("Gemini tool loop complete after {} rounds, {} tool calls", round + 1, total_tool_calls);
                return Ok(response);
            }

            tracing::info!(
                "Round {}: Gemini requested {} tool calls: {:?}",
                round + 1,
                response.tool_calls.len(),
                response.tool_calls.iter().map(|c| &c.name).collect::<Vec<_>>()
            );

            // Build model response with function calls (preserving thought signatures)
            let mut model_parts: Vec<GeminiPart> = vec![];

            // Map tool call IDs to thought signatures from raw_parts
            let mut thought_sigs: std::collections::HashMap<String, Option<String>> = std::collections::HashMap::new();
            for (idx, part) in raw_parts.iter().enumerate() {
                if part.function_call.is_some() {
                    let call_id = format!("gemini_{}", idx);
                    thought_sigs.insert(call_id, part.thought_signature.clone());
                }
            }

            // Add function call parts with thought signatures
            for call in &response.tool_calls {
                let thought_sig = thought_sigs.get(&call.id).cloned().flatten();
                model_parts.push(GeminiPart::FunctionCall(GeminiFunctionCallPart {
                    function_call: GeminiFunctionCall {
                        name: call.name.clone(),
                        args: call.arguments.clone(),
                    },
                    thought_signature: thought_sig,
                }));
            }

            // Add model's function call response
            contents.push(GeminiContent {
                role: "model".to_string(),
                parts: model_parts,
            });

            // Execute tools and build function responses
            let mut response_parts: Vec<GeminiPart> = vec![];
            for call in &response.tool_calls {
                let tool_call = tool_bridge::ToolCall {
                    id: call.id.clone(),
                    name: call.name.clone(),
                    arguments: call.arguments.clone(),
                };
                let result = tool_bridge::execute_tool(ctx, &tool_call).await;
                total_tool_calls += 1;

                response_parts.push(GeminiPart::FunctionResponse(GeminiFunctionResponsePart {
                    function_response: GeminiFunctionResponse {
                        name: call.name.clone(),
                        response: serde_json::json!({ "result": result.content }),
                    },
                }));
            }

            // Add user turn with function responses
            contents.push(GeminiContent {
                role: "user".to_string(),
                parts: response_parts,
            });

            // Check if we've hit budget limits
            if !ctx.tracker.can_call(&ctx.budget) {
                tracing::warn!("Tool budget exhausted after {} calls", total_tool_calls);
                // Do one more call without tools to get final response
                let (final_response, _) = gemini.complete_with_contents(
                    contents,
                    system,
                    false,
                ).await?;
                return Ok(final_response);
            }
        }

        // If we hit max rounds, do a final call without tools
        tracing::warn!("Hit max tool rounds ({}), forcing final response", MAX_TOOL_ROUNDS);
        let (final_response, _) = gemini.complete_with_contents(
            contents,
            system,
            false,
        ).await?;

        Ok(final_response)
    }

    /// DeepSeek Reasoner tool loop using Chat Completions API
    ///
    /// Has an overall timeout of 3 minutes (reasoner is slower due to reasoning).
    async fn ask_with_tools_deepseek(
        &self,
        message: &str,
        system: Option<String>,
        ctx: &mut tool_bridge::ToolContext,
    ) -> Result<AdvisoryResponse> {
        use std::time::Duration;
        use tokio::time::timeout;

        // Overall timeout for the entire tool loop (3 minutes for reasoner)
        const TOOL_LOOP_TIMEOUT_SECS: u64 = 180;

        timeout(
            Duration::from_secs(TOOL_LOOP_TIMEOUT_SECS),
            self.ask_with_tools_deepseek_inner(message, system, ctx),
        )
        .await
        .map_err(|_| anyhow::anyhow!("DeepSeek tool loop timed out after {} seconds", TOOL_LOOP_TIMEOUT_SECS))?
    }

    /// Inner implementation of DeepSeek Reasoner tool loop
    async fn ask_with_tools_deepseek_inner(
        &self,
        message: &str,
        system: Option<String>,
        ctx: &mut tool_bridge::ToolContext,
    ) -> Result<AdvisoryResponse> {
        let reasoner = ReasonerProvider::from_env()?;

        const MAX_TOOL_ROUNDS: usize = 5;
        let mut total_tool_calls = 0;

        // Build initial request with tools enabled
        let mut request = AdvisoryRequest::with_tools(message.to_string());
        request.system = system.clone();

        tracing::info!("Starting DeepSeek tool loop for: {}...", &message[..message.len().min(50)]);

        for round in 0..MAX_TOOL_ROUNDS {
            ctx.tracker.new_call();

            let round_start = std::time::Instant::now();
            tracing::info!("DeepSeek tool loop round {} starting...", round + 1);

            let response = reasoner.complete(request.clone()).await?;

            let elapsed = round_start.elapsed();
            tracing::info!("DeepSeek round {} API call took {:?}", round + 1, elapsed);

            // If no tool calls, we're done
            if response.tool_calls.is_empty() {
                tracing::info!("DeepSeek tool loop complete after {} rounds, {} tool calls", round + 1, total_tool_calls);
                return Ok(response);
            }

            tracing::info!(
                "Round {}: DeepSeek requested {} tool calls: {:?}",
                round + 1,
                response.tool_calls.len(),
                response.tool_calls.iter().map(|c| &c.name).collect::<Vec<_>>()
            );

            // Execute tools and collect results
            let mut tool_results = String::new();
            for call in &response.tool_calls {
                let tool_call = tool_bridge::ToolCall {
                    id: call.id.clone(),
                    name: call.name.clone(),
                    arguments: call.arguments.clone(),
                };
                let result = tool_bridge::execute_tool(ctx, &tool_call).await;
                total_tool_calls += 1;

                tool_results.push_str(&format!("\n[Tool: {}]\n{}\n", call.name, result.content));
            }

            // Add the assistant response and tool results to history
            let assistant_msg = AdvisoryMessage {
                role: AdvisoryRole::Assistant,
                content: if response.text.is_empty() {
                    format!("[Called tools: {}]", response.tool_calls.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join(", "))
                } else {
                    response.text.clone()
                },
            };
            request.history.push(assistant_msg);

            // Add tool results as user message
            let tool_msg = AdvisoryMessage {
                role: AdvisoryRole::User,
                content: format!("Tool results:{}\n\nNow provide your response based on the above information.", tool_results),
            };
            request.history.push(tool_msg);

            // Check if we've hit budget limits
            if !ctx.tracker.can_call(&ctx.budget) {
                tracing::warn!("Tool budget exhausted after {} calls", total_tool_calls);
                // Do one more call without tools to get final response
                request.enable_tools = false;
                let final_response = reasoner.complete(request).await?;
                return Ok(final_response);
            }
        }

        // If we hit max rounds, do a final call without tools
        tracing::warn!("Hit max tool rounds ({}), forcing final response", MAX_TOOL_ROUNDS);
        request.enable_tools = false;
        let final_response = reasoner.complete(request).await?;

        Ok(final_response)
    }

    /// Council query - multiple models in parallel, synthesized by Reasoner
    pub async fn council(
        &self,
        message: &str,
        exclude_model: Option<AdvisoryModel>,
    ) -> Result<CouncilResponse> {
        // Determine which models to query (exclude host model if specified)
        let council_models: Vec<AdvisoryModel> = [
            AdvisoryModel::Gpt52,
            AdvisoryModel::Opus45,
            AdvisoryModel::Gemini3Pro,
        ].into_iter()
            .filter(|m| Some(*m) != exclude_model)
            .filter(|m| self.providers.contains_key(m))
            .collect();

        if council_models.is_empty() {
            anyhow::bail!("No council models available");
        }

        // Query all council members in parallel
        let futures: Vec<_> = council_models.iter().map(|model| {
            let provider = self.providers.get(model).unwrap().clone();
            let msg = message.to_string();
            async move {
                let result = provider.complete(AdvisoryRequest {
                    message: msg,
                    system: None,
                    history: vec![],
                    enable_tools: false,
                }).await;
                (*model, result)
            }
        }).collect();

        let results: Vec<_> = futures::future::join_all(futures).await;

        // Collect responses
        let mut raw_responses: HashMap<AdvisoryModel, String> = HashMap::new();
        let mut errors: Vec<String> = vec![];

        for (model, result) in results {
            match result {
                Ok(response) => {
                    raw_responses.insert(model, response.text);
                }
                Err(e) => {
                    errors.push(format!("{:?}: {}", model, e));
                }
            }
        }

        if raw_responses.is_empty() {
            anyhow::bail!("All council members failed: {:?}", errors);
        }

        // Synthesize using DeepSeek Reasoner
        let (synthesis_raw, synthesis) = if let Some(reasoner) = self.providers.get(&AdvisoryModel::DeepSeekReasoner) {
            let synthesis_prompt = synthesis::build_synthesis_prompt(&raw_responses, message);
            match reasoner.complete(AdvisoryRequest {
                message: synthesis_prompt,
                system: Some(synthesis::SYNTHESIS_SYSTEM_PROMPT.to_string()),
                history: vec![],
                enable_tools: false,
            }).await {
                Ok(response) => {
                    let raw = response.text;
                    // Try to parse as structured JSON
                    let parsed = match CouncilSynthesis::parse(&raw) {
                        Ok(s) => s,
                        Err(e) => {
                            // Parsing failed, fall back to raw text
                            tracing::warn!("Failed to parse synthesis JSON: {}", e);
                            CouncilSynthesis::from_raw_text(&raw)
                        }
                    };
                    (Some(raw), Some(parsed))
                }
                Err(e) => {
                    errors.push(format!("Synthesis failed: {}", e));
                    (None, None)
                }
            }
        } else {
            (None, None)
        };

        Ok(CouncilResponse {
            raw_responses,
            synthesis_raw,
            synthesis,
            errors: if errors.is_empty() { None } else { Some(errors) },
        })
    }

    /// Check which providers are available
    #[allow(dead_code)]
    pub fn available_models(&self) -> Vec<AdvisoryModel> {
        self.providers.keys().copied().collect()
    }

    /// Council query without synthesis - returns raw responses only
    /// Use this when the host model will synthesize inline (e.g., chat on DeepSeek Reasoner)
    pub async fn council_raw(
        &self,
        message: &str,
        exclude_model: Option<AdvisoryModel>,
    ) -> Result<std::collections::HashMap<AdvisoryModel, String>> {
        // Determine which models to query (exclude host model if specified)
        let council_models: Vec<AdvisoryModel> = [
            AdvisoryModel::Gpt52,
            AdvisoryModel::Opus45,
            AdvisoryModel::Gemini3Pro,
        ].into_iter()
            .filter(|m| Some(*m) != exclude_model)
            .filter(|m| self.providers.contains_key(m))
            .collect();

        if council_models.is_empty() {
            anyhow::bail!("No council models available");
        }

        // Query all council members in parallel
        let futures: Vec<_> = council_models.iter().map(|model| {
            let provider = self.providers.get(model).unwrap().clone();
            let msg = message.to_string();
            async move {
                let result = provider.complete(AdvisoryRequest {
                    message: msg,
                    system: None,
                    history: vec![],
                    enable_tools: false,
                }).await;
                (*model, result)
            }
        }).collect();

        let results: Vec<_> = futures::future::join_all(futures).await;

        // Collect responses
        let mut responses: std::collections::HashMap<AdvisoryModel, String> = std::collections::HashMap::new();

        for (model, result) in results {
            match result {
                Ok(response) => {
                    responses.insert(model, response.text);
                }
                Err(e) => {
                    responses.insert(model, format!("(error: {})", e));
                }
            }
        }

        Ok(responses)
    }

    /// Council query with timeout handling - returns partial results if some providers timeout
    ///
    /// This version uses configurable timeouts and returns a StreamingCouncilResult
    /// that indicates which providers succeeded, timed out, or errored.
    #[allow(dead_code)]
    pub async fn council_with_timeout(
        &self,
        message: &str,
        exclude_model: Option<AdvisoryModel>,
        timeout_secs: Option<u64>,
    ) -> Result<StreamingCouncilResult> {
        use std::time::Duration;
        use tokio::time::timeout;

        let per_model_timeout = Duration::from_secs(timeout_secs.unwrap_or(60));

        // Determine which models to query
        let council_models: Vec<AdvisoryModel> = [
            AdvisoryModel::Gpt52,
            AdvisoryModel::Opus45,
            AdvisoryModel::Gemini3Pro,
        ].into_iter()
            .filter(|m| Some(*m) != exclude_model)
            .filter(|m| self.providers.contains_key(m))
            .collect();

        if council_models.is_empty() {
            anyhow::bail!("No council models available");
        }

        let mut result = StreamingCouncilResult::new();

        // Query all council members in parallel with individual timeouts
        let futures: Vec<_> = council_models.iter().map(|model| {
            let provider = self.providers.get(model).unwrap().clone();
            let msg = message.to_string();
            let model = *model;
            async move {
                let query_result = timeout(
                    per_model_timeout,
                    provider.complete(AdvisoryRequest {
                        message: msg,
                        system: None,
                        history: vec![],
                        enable_tools: false,
                    }),
                ).await;

                (model, query_result)
            }
        }).collect();

        let results = futures::future::join_all(futures).await;

        // Collect results, separating successes, timeouts, and errors
        for (model, query_result) in results {
            match query_result {
                Ok(Ok(response)) => {
                    result.responses.insert(model, response.text);
                }
                Ok(Err(e)) => {
                    result.errors.insert(model, format!("{}", e));
                }
                Err(_) => {
                    // Timeout
                    result.timeouts.push(model);
                }
            }
        }

        Ok(result)
    }

    /// Streaming council query with progress updates
    ///
    /// Sends progress events to the provided channel as each model responds.
    /// Returns the final result with all responses.
    #[allow(dead_code)]
    pub async fn council_streaming(
        &self,
        message: &str,
        exclude_model: Option<AdvisoryModel>,
        progress_tx: tokio::sync::mpsc::Sender<streaming::CouncilProgress>,
    ) -> Result<StreamingCouncilResult> {
        use std::time::Duration;
        use tokio::time::timeout;
        use tokio::sync::mpsc;

        let per_model_timeout = Duration::from_secs(60);

        // Determine which models to query
        let council_models: Vec<AdvisoryModel> = [
            AdvisoryModel::Gpt52,
            AdvisoryModel::Opus45,
            AdvisoryModel::Gemini3Pro,
        ].into_iter()
            .filter(|m| Some(*m) != exclude_model)
            .filter(|m| self.providers.contains_key(m))
            .collect();

        if council_models.is_empty() {
            anyhow::bail!("No council models available");
        }

        let mut result = StreamingCouncilResult::new();

        // Query all council members in parallel with streaming
        let futures: Vec<_> = council_models.iter().map(|model| {
            let provider = self.providers.get(model).unwrap().clone();
            let msg = message.to_string();
            let model = *model;
            let progress = progress_tx.clone();

            async move {
                // Notify that this model started
                let _ = progress.send(streaming::CouncilProgress::ModelStarted(model)).await;

                // Create a channel for streaming events
                let (tx, mut rx) = mpsc::channel::<AdvisoryEvent>(100);

                // Spawn the streaming task
                let stream_handle = tokio::spawn({
                    let provider = provider.clone();
                    async move {
                        provider.stream(AdvisoryRequest {
                            message: msg,
                            system: None,
                            history: vec![],
                            enable_tools: false,
                        }, tx).await
                    }
                });

                // Forward deltas to progress channel
                let progress_clone = progress.clone();
                let forward_handle = tokio::spawn(async move {
                    while let Some(event) = rx.recv().await {
                        match event {
                            AdvisoryEvent::TextDelta(delta) => {
                                let _ = progress_clone.send(
                                    streaming::CouncilProgress::ModelDelta { model, delta }
                                ).await;
                            }
                            AdvisoryEvent::Done => break,
                            _ => {}
                        }
                    }
                });

                // Wait for streaming with timeout
                let query_result = timeout(per_model_timeout, stream_handle).await;

                // Wait for forwarding to complete
                let _ = forward_handle.await;

                match query_result {
                    Ok(Ok(Ok(text))) => {
                        let _ = progress.send(
                            streaming::CouncilProgress::ModelCompleted { model, text: text.clone() }
                        ).await;
                        (model, Ok(text))
                    }
                    Ok(Ok(Err(e))) => {
                        let error = format!("{}", e);
                        let _ = progress.send(
                            streaming::CouncilProgress::ModelError { model, error: error.clone() }
                        ).await;
                        (model, Err(error))
                    }
                    Ok(Err(e)) => {
                        let error = format!("Task panic: {}", e);
                        let _ = progress.send(
                            streaming::CouncilProgress::ModelError { model, error: error.clone() }
                        ).await;
                        (model, Err(error))
                    }
                    Err(_) => {
                        let _ = progress.send(
                            streaming::CouncilProgress::ModelTimeout(model)
                        ).await;
                        (model, Err("Timeout".to_string()))
                    }
                }
            }
        }).collect();

        let results = futures::future::join_all(futures).await;

        // Collect final results
        for (model, query_result) in results {
            match query_result {
                Ok(text) => {
                    result.responses.insert(model, text);
                }
                Err(e) if e == "Timeout" => {
                    result.timeouts.push(model);
                }
                Err(e) => {
                    result.errors.insert(model, e);
                }
            }
        }

        // Send final done event
        let _ = progress_tx.send(streaming::CouncilProgress::Done(result.clone())).await;

        Ok(result)
    }
}

/// Response from council query
#[derive(Debug, Clone)]
pub struct CouncilResponse {
    /// Individual model responses
    pub raw_responses: HashMap<AdvisoryModel, String>,
    /// Raw synthesis text from DeepSeek Reasoner
    pub synthesis_raw: Option<String>,
    /// Parsed structured synthesis with provenance
    pub synthesis: Option<CouncilSynthesis>,
    /// Any errors that occurred
    pub errors: Option<Vec<String>>,
}

impl CouncilResponse {
    /// Format as JSON for MCP response
    pub fn to_json(&self) -> serde_json::Value {
        let mut result = serde_json::json!({});

        // Add individual responses
        let mut council = serde_json::json!({});
        for (model, response) in &self.raw_responses {
            council[model.as_str()] = serde_json::Value::String(response.clone());
        }
        result["council"] = council;

        // Add structured synthesis if available
        if let Some(synthesis) = &self.synthesis {
            result["synthesis"] = synthesis.to_json();
            // Also include markdown version for easy reading
            result["synthesis_markdown"] = serde_json::Value::String(synthesis.to_markdown());
        }

        // Include raw synthesis for debugging
        if let Some(raw) = &self.synthesis_raw {
            result["synthesis_raw"] = serde_json::Value::String(raw.clone());
        }

        // Add errors if any
        if let Some(errors) = &self.errors {
            result["errors"] = serde_json::json!(errors);
        }

        result
    }

    /// Get a human-readable synthesis summary
    #[allow(dead_code)]
    pub fn synthesis_markdown(&self) -> Option<String> {
        self.synthesis.as_ref().map(|s| s.to_markdown())
    }

    /// Check if synthesis has high confidence
    #[allow(dead_code)]
    pub fn is_high_confidence(&self) -> bool {
        self.synthesis.as_ref()
            .map(|s| s.confidence == SynthesisConfidence::High)
            .unwrap_or(false)
    }
}

