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

use crate::core::primitives::semantic::SemanticSearch;

/// Truncate a string to a maximum length, ending at word boundary with ellipsis
fn truncate_to_snippet(s: &str, max_chars: usize) -> String {
    if s.len() <= max_chars {
        return s.to_string();
    }

    // Find a good break point (space, newline) before max_chars
    let truncated = &s[..max_chars];
    if let Some(pos) = truncated.rfind(|c: char| c.is_whitespace()) {
        format!("{}…", &s[..pos].trim())
    } else {
        format!("{}…", truncated)
    }
}

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
                tool_usage: HashMap::new(), // No tool support in legacy council_deliberate
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
    ///
    /// After synthesis, updates the session topic with the generated title.
    pub async fn council_deliberate_with_progress(
        &self,
        message: &str,
        config: Option<DeliberationConfig>,
        db: &sqlx::SqlitePool,
        semantic: &Arc<SemanticSearch>,
        project_id: Option<i64>,
        session_id: &str,
    ) -> Result<DeliberatedSynthesis> {
        use std::time::Duration;
        use tokio::time::timeout;
        use session::{DeliberationProgress, update_deliberation_progress, update_topic};
        use tool_bridge::SharedToolBudget;
        use deliberation::ToolCallRecord;

        let config = config.unwrap_or_default();
        let mut rounds: Vec<DeliberationRound> = Vec::new();
        let mut previous_responses: HashMap<AdvisoryModel, Vec<String>> = HashMap::new();
        let mut moderator_analyses: Vec<ModeratorAnalysis> = Vec::new();

        // Create shared tool budget for coordinated tool usage across models
        let shared_budget = if config.enable_tools {
            Some(SharedToolBudget::new(
                Arc::new(db.clone()),
                semantic.clone(),
                project_id,
                config.tool_budget.clone(),
            ))
        } else {
            None
        };

        // Initialize progress
        let mut progress = DeliberationProgress::new(config.max_rounds);
        let _ = update_deliberation_progress(db, session_id, &progress).await;

        tracing::info!(
            max_rounds = config.max_rounds,
            models = ?config.models,
            enable_tools = config.enable_tools,
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
            // Use longer timeout when tools are enabled (tool loops take more time)
            let per_model_timeout = Duration::from_secs(
                if config.enable_tools { 90 } else { config.per_model_timeout_secs }
            );
            let mut round_responses: HashMap<AdvisoryModel, String> = HashMap::new();
            let mut round_errors: Vec<String> = Vec::new();
            let mut round_tool_usage: HashMap<String, Vec<ToolCallRecord>> = HashMap::new();

            if config.enable_tools {
                // Tool-enabled path: use tool loops for each model
                if let Some(ref budget) = shared_budget {
                    let futures: Vec<_> = config.models.iter().map(|model| {
                        let model = *model;
                        let prompt = prompt.clone();
                        let system_prompt = deliberation::build_deliberation_system_prompt(model);
                        let mut ctx = budget.model_context();

                        async move {
                            let result = timeout(
                                per_model_timeout,
                                async {
                                    match model {
                                        AdvisoryModel::Gpt52 => {
                                            tool_loops::ask_with_tools_gpt(&prompt, Some(system_prompt), &mut ctx).await
                                        }
                                        AdvisoryModel::Gemini3Pro => {
                                            tool_loops::ask_with_tools_gemini(&prompt, Some(system_prompt), &mut ctx).await
                                        }
                                        AdvisoryModel::Opus45 => {
                                            tool_loops::ask_with_tools_opus(&prompt, Some(system_prompt), &mut ctx).await
                                        }
                                        _ => anyhow::bail!("Model {:?} not supported in council", model),
                                    }
                                }
                            ).await;

                            // Extract tool usage from context
                            let tool_records: Vec<ToolCallRecord> = ctx.tracker.recent_queries
                                .iter()
                                .map(|(fingerprint, _)| ToolCallRecord {
                                    tool_name: fingerprint.split(':').next().unwrap_or("unknown").to_string(),
                                    query_summary: fingerprint.chars().take(50).collect(),
                                    success: true,
                                })
                                .collect();

                            (model, result, tool_records, ctx)
                        }
                    }).collect();

                    let results = futures::future::join_all(futures).await;

                    for (model, result, tool_records, ctx) in results {
                        // Merge usage back into shared budget
                        budget.merge_usage(&ctx);

                        match result {
                            Ok(Ok(response)) => {
                                tracing::debug!(model = ?model, tools_used = tool_records.len(), "Model responded with tools");
                                round_responses.insert(model, response.text.clone());
                                previous_responses.entry(model).or_default().push(response.text);
                                if !tool_records.is_empty() {
                                    round_tool_usage.insert(model.as_str().to_string(), tool_records);
                                }

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
                }
            } else {
                // Non-tool path: use direct provider.complete()
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
                tool_usage: round_tool_usage,
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

        // Update session topic from synthesized title
        if let Some(title) = &result.synthesis.session_title {
            if let Err(e) = update_topic(db, session_id, title).await {
                tracing::warn!(error = %e, "Failed to update session topic");
            }
        }

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
                let model_str = model.as_str().to_string();

                // Notify that this model started
                let _ = progress.send(streaming::CouncilProgress::ModelStarted {
                    model: model_str.clone()
                }).await;

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
                let model_str_clone = model_str.clone();
                let forward_handle = tokio::spawn(async move {
                    while let Some(event) = rx.recv().await {
                        match event {
                            AdvisoryEvent::TextDelta(delta) => {
                                let _ = progress_clone.send(
                                    streaming::CouncilProgress::ModelDelta {
                                        model: model_str_clone.clone(),
                                        delta
                                    }
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
                            streaming::CouncilProgress::ModelCompleted {
                                model: model_str.clone(),
                                text: text.clone(),
                                reasoning_snippet: None, // Streaming mode - reasoning captured via ReasoningDelta
                            }
                        ).await;
                        (model, Ok(text))
                    }
                    Ok(Ok(Err(e))) => {
                        let error = format!("{}", e);
                        let _ = progress.send(
                            streaming::CouncilProgress::ModelError {
                                model: model_str.clone(),
                                error: error.clone()
                            }
                        ).await;
                        (model, Err(error))
                    }
                    Ok(Err(e)) => {
                        let error = format!("Task panic: {}", e);
                        let _ = progress.send(
                            streaming::CouncilProgress::ModelError {
                                model: model_str.clone(),
                                error: error.clone()
                            }
                        ).await;
                        (model, Err(error))
                    }
                    Err(_) => {
                        let _ = progress.send(
                            streaming::CouncilProgress::ModelTimeout {
                                model: model_str.clone()
                            }
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

        // Send final done event - convert result to JSON
        let result_json = serde_json::json!({
            "responses": result.responses.iter()
                .map(|(k, v)| (k.as_str(), v.clone()))
                .collect::<HashMap<&str, String>>(),
            "timeouts": result.timeouts.iter().map(|m| m.as_str()).collect::<Vec<_>>(),
            "errors": result.errors.iter()
                .map(|(k, v)| (k.as_str(), v.clone()))
                .collect::<HashMap<&str, String>>(),
        });
        let _ = progress_tx.send(streaming::CouncilProgress::Done { result: result_json }).await;

        Ok(result)
    }

    /// Multi-round council deliberation with SSE streaming
    ///
    /// Like `council_deliberate_with_progress` but sends real-time events
    /// to a channel for SSE streaming to the frontend.
    pub async fn council_deliberate_streaming(
        &self,
        message: &str,
        config: Option<DeliberationConfig>,
        db: &sqlx::SqlitePool,
        semantic: &Arc<SemanticSearch>,
        project_id: Option<i64>,
        session_id: &str,
        progress_tx: tokio::sync::mpsc::Sender<streaming::CouncilProgress>,
    ) -> Result<DeliberatedSynthesis> {
        use std::time::Duration;
        use tokio::time::timeout;
        use session::{DeliberationProgress, update_deliberation_progress, update_topic};
        use tool_bridge::SharedToolBudget;
        use deliberation::ToolCallRecord;

        let config = config.unwrap_or_default();
        let mut rounds: Vec<DeliberationRound> = Vec::new();
        let mut previous_responses: HashMap<AdvisoryModel, Vec<String>> = HashMap::new();
        let mut moderator_analyses: Vec<ModeratorAnalysis> = Vec::new();

        // Create shared tool budget for coordinated tool usage across models
        let shared_budget = if config.enable_tools {
            Some(SharedToolBudget::new(
                Arc::new(db.clone()),
                semantic.clone(),
                project_id,
                config.tool_budget.clone(),
            ))
        } else {
            None
        };

        // Initialize progress
        let mut progress = DeliberationProgress::new(config.max_rounds);
        let _ = update_deliberation_progress(db, session_id, &progress).await;

        tracing::info!(
            max_rounds = config.max_rounds,
            models = ?config.models,
            enable_tools = config.enable_tools,
            session_id = session_id,
            "Starting streaming council deliberation"
        );

        for round_num in 1..=config.max_rounds {
            // Update progress: starting new round
            progress.start_round(round_num);
            let _ = update_deliberation_progress(db, session_id, &progress).await;

            // Send SSE event
            let _ = progress_tx.send(streaming::CouncilProgress::RoundStarted {
                round: round_num,
                max_rounds: config.max_rounds,
            }).await;

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
            // Use longer timeout when tools are enabled
            let per_model_timeout = Duration::from_secs(
                if config.enable_tools { 90 } else { config.per_model_timeout_secs }
            );
            let mut round_responses: HashMap<AdvisoryModel, String> = HashMap::new();
            let mut round_errors: Vec<String> = Vec::new();
            let mut round_tool_usage: HashMap<String, Vec<ToolCallRecord>> = HashMap::new();

            if config.enable_tools {
                // Tool-enabled path with SSE events
                if let Some(ref budget) = shared_budget {
                    let futures: Vec<_> = config.models.iter().map(|model| {
                        let model = *model;
                        let prompt = prompt.clone();
                        let system_prompt = deliberation::build_deliberation_system_prompt(model);
                        let mut ctx = budget.model_context();
                        let progress_tx = progress_tx.clone();

                        async move {
                            let model_str = model.as_str().to_string();

                            // Send model started event
                            let _ = progress_tx.send(streaming::CouncilProgress::ModelStarted {
                                model: model_str.clone()
                            }).await;

                            let result = timeout(
                                per_model_timeout,
                                async {
                                    match model {
                                        AdvisoryModel::Gpt52 => {
                                            tool_loops::ask_with_tools_gpt(&prompt, Some(system_prompt), &mut ctx).await
                                        }
                                        AdvisoryModel::Gemini3Pro => {
                                            tool_loops::ask_with_tools_gemini(&prompt, Some(system_prompt), &mut ctx).await
                                        }
                                        AdvisoryModel::Opus45 => {
                                            tool_loops::ask_with_tools_opus(&prompt, Some(system_prompt), &mut ctx).await
                                        }
                                        _ => anyhow::bail!("Model {:?} not supported in council", model),
                                    }
                                }
                            ).await;

                            // Extract tool usage from context
                            let tool_records: Vec<ToolCallRecord> = ctx.tracker.recent_queries
                                .iter()
                                .map(|(fingerprint, _)| ToolCallRecord {
                                    tool_name: fingerprint.split(':').next().unwrap_or("unknown").to_string(),
                                    query_summary: fingerprint.chars().take(50).collect(),
                                    success: true,
                                })
                                .collect();

                            // Send tool completion event if tools were used
                            if !tool_records.is_empty() {
                                let tools_called: Vec<String> = tool_records.iter()
                                    .map(|t| t.tool_name.clone())
                                    .collect();
                                let _ = progress_tx.send(streaming::CouncilProgress::ModelToolsComplete {
                                    model: model_str.clone(),
                                    tools_called,
                                    round: round_num,
                                }).await;
                            }

                            match result {
                                Ok(Ok(response)) => {
                                    let reasoning_snippet = response.reasoning.as_ref()
                                        .map(|r| truncate_to_snippet(r, 500))
                                        .or_else(|| Some(truncate_to_snippet(&response.text, 500)));

                                    let _ = progress_tx.send(streaming::CouncilProgress::ModelCompleted {
                                        model: model_str,
                                        text: response.text.clone(),
                                        reasoning_snippet,
                                    }).await;
                                    (model, Ok(response.text), tool_records, ctx)
                                }
                                Ok(Err(e)) => {
                                    let error = format!("{}", e);
                                    let _ = progress_tx.send(streaming::CouncilProgress::ModelError {
                                        model: model_str,
                                        error: error.clone(),
                                    }).await;
                                    (model, Err(error), tool_records, ctx)
                                }
                                Err(_) => {
                                    let _ = progress_tx.send(streaming::CouncilProgress::ModelTimeout {
                                        model: model_str,
                                    }).await;
                                    (model, Err("Timeout".to_string()), tool_records, ctx)
                                }
                            }
                        }
                    }).collect();

                    let results = futures::future::join_all(futures).await;

                    for (model, result, tool_records, ctx) in results {
                        budget.merge_usage(&ctx);

                        match result {
                            Ok(text) => {
                                tracing::debug!(model = ?model, tools_used = tool_records.len(), "Model responded with tools");
                                round_responses.insert(model, text.clone());
                                previous_responses.entry(model).or_default().push(text);
                                if !tool_records.is_empty() {
                                    round_tool_usage.insert(model.as_str().to_string(), tool_records);
                                }

                                progress.model_responded(model.as_str());
                                let _ = update_deliberation_progress(db, session_id, &progress).await;
                            }
                            Err(e) => {
                                tracing::warn!(model = ?model, error = %e, "Model failed");
                                round_errors.push(format!("{:?}: {}", model, e));
                            }
                        }
                    }
                }
            } else {
                // Non-tool path
                let futures: Vec<_> = config.models.iter().filter_map(|model| {
                    self.providers.get(model).map(|provider| {
                        let provider = provider.clone();
                        let prompt = prompt.clone();
                        let model = *model;
                        let system_prompt = deliberation::build_deliberation_system_prompt(model);
                        let progress_tx = progress_tx.clone();

                        async move {
                            let model_str = model.as_str().to_string();

                            let _ = progress_tx.send(streaming::CouncilProgress::ModelStarted {
                                model: model_str.clone()
                            }).await;

                            let result = timeout(
                                per_model_timeout,
                                provider.complete(AdvisoryRequest {
                                    message: prompt,
                                    system: Some(system_prompt),
                                    history: vec![],
                                    enable_tools: false,
                                }),
                            ).await;

                            match result {
                                Ok(Ok(response)) => {
                                    let reasoning_snippet = response.reasoning.as_ref()
                                        .map(|r| truncate_to_snippet(r, 500))
                                        .or_else(|| Some(truncate_to_snippet(&response.text, 500)));

                                    let _ = progress_tx.send(streaming::CouncilProgress::ModelCompleted {
                                        model: model_str,
                                        text: response.text.clone(),
                                        reasoning_snippet,
                                    }).await;
                                    (model, Ok(response.text))
                                }
                                Ok(Err(e)) => {
                                    let error = format!("{}", e);
                                    let _ = progress_tx.send(streaming::CouncilProgress::ModelError {
                                        model: model_str,
                                        error: error.clone(),
                                    }).await;
                                    (model, Err(error))
                                }
                                Err(_) => {
                                    let _ = progress_tx.send(streaming::CouncilProgress::ModelTimeout {
                                        model: model_str,
                                    }).await;
                                    (model, Err("Timeout".to_string()))
                                }
                            }
                        }
                    })
                }).collect();

                let results = futures::future::join_all(futures).await;

                for (model, result) in results {
                    match result {
                        Ok(text) => {
                            tracing::debug!(model = ?model, "Model responded");
                            round_responses.insert(model, text.clone());
                            previous_responses.entry(model).or_default().push(text);

                            progress.model_responded(model.as_str());
                            let _ = update_deliberation_progress(db, session_id, &progress).await;
                        }
                        Err(e) => {
                            tracing::warn!(model = ?model, error = %e, "Model failed");
                            round_errors.push(format!("{:?}: {}", model, e));
                        }
                    }
                }
            }

            if round_responses.is_empty() {
                let error = format!("All models failed in round {}: {:?}", round_num, round_errors);
                progress.fail(error.clone());
                let _ = update_deliberation_progress(db, session_id, &progress).await;
                let _ = progress_tx.send(streaming::CouncilProgress::DeliberationFailed {
                    error: error.clone()
                }).await;
                anyhow::bail!(error);
            }

            // Update progress: round complete, starting moderator analysis
            progress.round_complete(round_num);
            let _ = update_deliberation_progress(db, session_id, &progress).await;

            // Send moderator analyzing event
            let _ = progress_tx.send(streaming::CouncilProgress::ModeratorAnalyzing {
                round: round_num
            }).await;

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

                                    // Send moderator complete event
                                    let _ = progress_tx.send(streaming::CouncilProgress::ModeratorComplete {
                                        round: round_num,
                                        should_continue: a.should_continue,
                                        disagreements: a.disagreements.iter().map(|d| d.topic.clone()).collect(),
                                        focus_questions: a.focus_questions.clone(),
                                        resolved_points: a.resolved_points.clone(),
                                    }).await;

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
                tool_usage: round_tool_usage,
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

                    // Send early consensus event
                    let _ = progress_tx.send(streaming::CouncilProgress::EarlyConsensus {
                        round: round_num,
                        reason: a.early_exit_reason.clone(),
                    }).await;

                    moderator_analyses.push(a.clone());
                    break;
                }
                moderator_analyses.push(a.clone());
            }
        }

        // Update progress: starting synthesis
        progress.start_synthesis();
        let _ = update_deliberation_progress(db, session_id, &progress).await;

        // Send synthesis started event
        let _ = progress_tx.send(streaming::CouncilProgress::SynthesisStarted).await;

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

        // Update session topic if synthesis includes one
        if let Some(title) = result.synthesis.session_title.as_ref() {
            let _ = update_topic(db, session_id, title).await;
        }

        // Send deliberation complete event
        let _ = progress_tx.send(streaming::CouncilProgress::DeliberationComplete {
            result: result.to_json()
        }).await;

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

