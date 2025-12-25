//! Advisory Module - Unified LLM advisory system
//!
//! Consolidates MCP hotline and chat council into a single abstraction with:
//! - Multiple providers (GPT-5.2, Opus 4.5, Gemini 3 Pro)
//! - DeepSeek Reasoner as synthesizer
//! - Multi-turn sessions with tiered memory
//! - Streaming responses
//! - Agentic tool calling (read-only)
//! - Shared context injection

mod providers;
mod tool_loops;
pub mod context;
pub mod deliberation;
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
    // Opus types for tool loop
    OpusInputItem, OpusToolUse, AnthropicResponseBlock,
};
#[allow(unused_imports)]
pub use synthesis::{
    CouncilSynthesis, ConsensusPoint, Citation, Disagreement,
    ModelPosition, UniqueInsight, SynthesisConfidence,
};
#[allow(unused_imports)]
pub use deliberation::{
    DeliberationConfig, DeliberationRound, ModeratorAnalysis,
    DisagreementFocus, DeliberatedSynthesis,
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
    /// Supports GPT-5.2 (Responses API), Gemini 3 Pro, DeepSeek Reasoner, and Opus 4.5.
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
            AdvisoryModel::Gpt52 => tool_loops::ask_with_tools_gpt(message, system, ctx).await,
            AdvisoryModel::Gemini3Pro => tool_loops::ask_with_tools_gemini(message, system, ctx).await,
            AdvisoryModel::DeepSeekReasoner => tool_loops::ask_with_tools_deepseek(message, system, ctx).await,
            AdvisoryModel::Opus45 => tool_loops::ask_with_tools_opus(message, system, ctx).await,
        }
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

    /// Multi-round council deliberation
    ///
    /// Models engage in actual discussion rather than just parallel responses:
    /// - Up to 4 rounds (stops early on consensus)
    /// - All 3 models (GPT-5.2, Gemini 3 Pro, Opus 4.5) always participate
    /// - DeepSeek Reasoner moderates between rounds
    /// - Cache-optimized prompts for cost efficiency
    pub async fn council_deliberate(
        &self,
        message: &str,
        config: Option<DeliberationConfig>,
    ) -> Result<DeliberatedSynthesis> {
        use std::time::Duration;
        use tokio::time::timeout;

        let config = config.unwrap_or_default();
        let mut rounds: Vec<DeliberationRound> = Vec::new();
        let mut previous_responses: HashMap<AdvisoryModel, Vec<String>> = HashMap::new();
        let mut moderator_analyses: Vec<ModeratorAnalysis> = Vec::new();

        tracing::info!(
            max_rounds = config.max_rounds,
            models = ?config.models,
            "Starting council deliberation"
        );

        for round_num in 1..=config.max_rounds {
            tracing::info!(round = round_num, "Starting deliberation round");

            // Build prompt for this round
            let prompt = deliberation::build_round_prompt(
                message,
                round_num,
                config.max_rounds,
                &previous_responses,
                moderator_analyses.last(),
            );

            // Query all models in parallel
            let per_model_timeout = Duration::from_secs(config.per_model_timeout_secs);
            let mut round_responses: HashMap<AdvisoryModel, String> = HashMap::new();
            let mut round_errors: Vec<String> = Vec::new();

            let futures: Vec<_> = config.models.iter().filter_map(|model| {
                self.providers.get(model).map(|provider| {
                    let provider = provider.clone();
                    let prompt = prompt.clone();
                    let model = *model;
                    // Each model gets its own identity-aware system prompt
                    let system_prompt = deliberation::build_deliberation_system_prompt(model);
                    async move {
                        let result = timeout(
                            per_model_timeout,
                            provider.complete(AdvisoryRequest {
                                message: prompt,
                                system: Some(system_prompt),
                                history: vec![],
                                enable_tools: false,
                            }),
                        ).await;
                        (model, result)
                    }
                })
            }).collect();

            let results = futures::future::join_all(futures).await;

            for (model, result) in results {
                match result {
                    Ok(Ok(response)) => {
                        tracing::debug!(model = ?model, "Model responded");
                        round_responses.insert(model, response.text.clone());
                        previous_responses.entry(model).or_default().push(response.text);
                    }
                    Ok(Err(e)) => {
                        tracing::warn!(model = ?model, error = %e, "Model error");
                        round_errors.push(format!("{:?}: {}", model, e));
                    }
                    Err(_) => {
                        tracing::warn!(model = ?model, "Model timeout");
                        round_errors.push(format!("{:?}: timeout", model));
                    }
                }
            }

            if round_responses.is_empty() {
                anyhow::bail!("All models failed in round {}: {:?}", round_num, round_errors);
            }

            // Get moderator analysis (skip for final round)
            let analysis = if round_num < config.max_rounds {
                if let Some(reasoner) = self.providers.get(&AdvisoryModel::DeepSeekReasoner) {
                    let moderator_prompt = deliberation::build_moderator_prompt(
                        message,
                        round_num,
                        &round_responses,
                        &moderator_analyses,
                    );

                    match reasoner.complete(AdvisoryRequest {
                        message: moderator_prompt,
                        system: Some(deliberation::MODERATOR_SYSTEM_PROMPT.to_string()),
                        history: vec![],
                        enable_tools: false,
                    }).await {
                        Ok(response) => {
                            match ModeratorAnalysis::parse(&response.text) {
                                Ok(a) => {
                                    tracing::info!(
                                        should_continue = a.should_continue,
                                        disagreements = a.disagreements.len(),
                                        resolved = a.resolved_points.len(),
                                        "Moderator analysis complete"
                                    );
                                    Some(a)
                                }
                                Err(e) => {
                                    tracing::warn!(error = %e, "Failed to parse moderator analysis");
                                    None
                                }
                            }
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "Moderator analysis failed");
                            None
                        }
                    }
                } else {
                    None
                }
            } else {
                None
            };

            // Convert responses to string keys for serialization
            let responses_str: HashMap<String, String> = round_responses
                .iter()
                .map(|(k, v)| (k.as_str().to_string(), v.clone()))
                .collect();

            rounds.push(DeliberationRound {
                round: round_num,
                responses: responses_str,
                moderator_analysis: analysis.clone(),
                timestamp: chrono::Utc::now().timestamp(),
            });

            // Check for early consensus
            if let Some(ref a) = analysis {
                if !a.should_continue {
                    tracing::info!(
                        round = round_num,
                        reason = ?a.early_exit_reason,
                        "Early consensus reached"
                    );
                    moderator_analyses.push(a.clone());
                    break;
                }
                moderator_analyses.push(a.clone());
            }
        }

        // Final synthesis with deliberation context
        let synthesis = self.synthesize_deliberation(message, &rounds).await?;
        let early_consensus = rounds.len() < config.max_rounds as usize;

        tracing::info!(
            rounds_completed = rounds.len(),
            early_consensus = early_consensus,
            "Deliberation complete"
        );

        Ok(DeliberatedSynthesis {
            synthesis,
            rounds_completed: rounds.len() as u8,
            early_consensus,
            rounds,
        })
    }

    /// Synthesize deliberation into final recommendation
    async fn synthesize_deliberation(
        &self,
        message: &str,
        rounds: &[DeliberationRound],
    ) -> Result<CouncilSynthesis> {
        let reasoner = self.providers.get(&AdvisoryModel::DeepSeekReasoner)
            .ok_or_else(|| anyhow::anyhow!("DeepSeek Reasoner not available for synthesis"))?;

        let synthesis_prompt = deliberation::build_deliberation_synthesis_prompt(message, rounds);

        let response = reasoner.complete(AdvisoryRequest {
            message: synthesis_prompt,
            system: Some(synthesis::SYNTHESIS_SYSTEM_PROMPT.to_string()),
            history: vec![],
            enable_tools: false,
        }).await?;

        // Try to parse as structured JSON, fall back to raw text
        match CouncilSynthesis::parse(&response.text) {
            Ok(s) => Ok(s),
            Err(e) => {
                tracing::warn!(error = %e, "Failed to parse synthesis JSON, using raw text");
                Ok(CouncilSynthesis::from_raw_text(&response.text))
            }
        }
    }

    /// Council deliberation with progress updates for async execution
    ///
    /// Updates progress in the database after each significant event:
    /// - Each model responding in a round
    /// - Moderator analysis complete
    /// - Synthesis starting
    /// - Completion or failure
    pub async fn council_deliberate_with_progress(
        &self,
        message: &str,
        config: Option<DeliberationConfig>,
        db: &sqlx::SqlitePool,
        session_id: &str,
    ) -> Result<DeliberatedSynthesis> {
        use std::time::Duration;
        use tokio::time::timeout;
        use session::{DeliberationProgress, update_deliberation_progress};

        let config = config.unwrap_or_default();
        let mut rounds: Vec<DeliberationRound> = Vec::new();
        let mut previous_responses: HashMap<AdvisoryModel, Vec<String>> = HashMap::new();
        let mut moderator_analyses: Vec<ModeratorAnalysis> = Vec::new();

        // Initialize progress
        let mut progress = DeliberationProgress::new(config.max_rounds);
        let _ = update_deliberation_progress(db, session_id, &progress).await;

        tracing::info!(
            max_rounds = config.max_rounds,
            models = ?config.models,
            session_id = session_id,
            "Starting async council deliberation"
        );

        for round_num in 1..=config.max_rounds {
            // Update progress: starting new round
            progress.start_round(round_num);
            let _ = update_deliberation_progress(db, session_id, &progress).await;

            tracing::info!(round = round_num, "Starting deliberation round");

            // Build prompt for this round
            let prompt = deliberation::build_round_prompt(
                message,
                round_num,
                config.max_rounds,
                &previous_responses,
                moderator_analyses.last(),
            );

            // Query all models in parallel
            let per_model_timeout = Duration::from_secs(config.per_model_timeout_secs);
            let mut round_responses: HashMap<AdvisoryModel, String> = HashMap::new();
            let mut round_errors: Vec<String> = Vec::new();

            let futures: Vec<_> = config.models.iter().filter_map(|model| {
                self.providers.get(model).map(|provider| {
                    let provider = provider.clone();
                    let prompt = prompt.clone();
                    let model = *model;
                    let system_prompt = deliberation::build_deliberation_system_prompt(model);
                    async move {
                        let result = timeout(
                            per_model_timeout,
                            provider.complete(AdvisoryRequest {
                                message: prompt,
                                system: Some(system_prompt),
                                history: vec![],
                                enable_tools: false,
                            }),
                        ).await;
                        (model, result)
                    }
                })
            }).collect();

            let results = futures::future::join_all(futures).await;

            for (model, result) in results {
                match result {
                    Ok(Ok(response)) => {
                        tracing::debug!(model = ?model, "Model responded");
                        round_responses.insert(model, response.text.clone());
                        previous_responses.entry(model).or_default().push(response.text);

                        // Update progress: model responded
                        progress.model_responded(model.as_str());
                        let _ = update_deliberation_progress(db, session_id, &progress).await;
                    }
                    Ok(Err(e)) => {
                        tracing::warn!(model = ?model, error = %e, "Model error");
                        round_errors.push(format!("{:?}: {}", model, e));
                    }
                    Err(_) => {
                        tracing::warn!(model = ?model, "Model timeout");
                        round_errors.push(format!("{:?}: timeout", model));
                    }
                }
            }

            if round_responses.is_empty() {
                let error = format!("All models failed in round {}: {:?}", round_num, round_errors);
                progress.fail(error.clone());
                let _ = update_deliberation_progress(db, session_id, &progress).await;
                anyhow::bail!(error);
            }

            // Update progress: round complete, starting moderator analysis
            progress.round_complete(round_num);
            let _ = update_deliberation_progress(db, session_id, &progress).await;

            // Get moderator analysis (skip for final round)
            let analysis = if round_num < config.max_rounds {
                if let Some(reasoner) = self.providers.get(&AdvisoryModel::DeepSeekReasoner) {
                    let moderator_prompt = deliberation::build_moderator_prompt(
                        message,
                        round_num,
                        &round_responses,
                        &moderator_analyses,
                    );

                    match reasoner.complete(AdvisoryRequest {
                        message: moderator_prompt,
                        system: Some(deliberation::MODERATOR_SYSTEM_PROMPT.to_string()),
                        history: vec![],
                        enable_tools: false,
                    }).await {
                        Ok(response) => {
                            match ModeratorAnalysis::parse(&response.text) {
                                Ok(a) => {
                                    tracing::info!(
                                        should_continue = a.should_continue,
                                        disagreements = a.disagreements.len(),
                                        resolved = a.resolved_points.len(),
                                        "Moderator analysis complete"
                                    );
                                    Some(a)
                                }
                                Err(e) => {
                                    tracing::warn!(error = %e, "Failed to parse moderator analysis");
                                    None
                                }
                            }
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "Moderator error");
                            None
                        }
                    }
                } else {
                    None
                }
            } else {
                None
            };

            // Record this round
            let now = chrono::Utc::now().timestamp();
            let round_data = DeliberationRound {
                round: round_num,
                responses: round_responses.iter()
                    .map(|(m, r)| (m.as_str().to_string(), r.clone()))
                    .collect(),
                moderator_analysis: analysis.clone(),
                timestamp: now,
            };
            rounds.push(round_data);

            // Check if we should continue
            if let Some(ref a) = analysis {
                if !a.should_continue {
                    tracing::info!(
                        round = round_num,
                        reason = ?a.early_exit_reason,
                        "Early consensus reached"
                    );
                    moderator_analyses.push(a.clone());
                    break;
                }
                moderator_analyses.push(a.clone());
            }
        }

        // Update progress: starting synthesis
        progress.start_synthesis();
        let _ = update_deliberation_progress(db, session_id, &progress).await;

        // Final synthesis with deliberation context
        let synthesis = self.synthesize_deliberation(message, &rounds).await?;
        let early_consensus = rounds.len() < config.max_rounds as usize;

        tracing::info!(
            rounds_completed = rounds.len(),
            early_consensus = early_consensus,
            "Deliberation complete"
        );

        let result = DeliberatedSynthesis {
            synthesis,
            rounds_completed: rounds.len() as u8,
            early_consensus,
            rounds,
        };

        // Update progress: complete with result
        progress.complete(result.to_json());
        let _ = update_deliberation_progress(db, session_id, &progress).await;

        Ok(result)
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

