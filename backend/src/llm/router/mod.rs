// src/llm/router/mod.rs
// Multi-model router for intelligent task routing

mod classifier;
pub mod config;
pub mod types;

use anyhow::Result;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use tracing::{debug, info, warn};

use super::provider::LlmProvider;

// Re-export public types
pub use classifier::TaskClassifier;
pub use config::RouterConfig;
pub use types::{ModelTier, RoutingStats, RoutingTask};

/// Multi-model router that routes tasks to appropriate providers
///
/// Tiers (all OpenAI):
/// - Fast: GPT-5.1 Mini - file ops, search, simple queries
/// - Voice: GPT-5.1 (low reasoning) - user chat, explanations, Mira's personality
/// - Thinker: GPT-5.1 (high reasoning) - complex reasoning, architecture
pub struct ModelRouter {
    /// Fast tier provider (GPT-5.1 Mini)
    fast_provider: Arc<dyn LlmProvider>,
    /// Voice tier provider (GPT-5.1)
    voice_provider: Arc<dyn LlmProvider>,
    /// Thinker tier provider (GPT-5.1 High)
    thinker_provider: Arc<dyn LlmProvider>,
    /// Task classifier
    classifier: TaskClassifier,
    /// Configuration
    config: RouterConfig,
    /// Statistics
    stats: RwLock<RoutingStats>,
    /// Request counter for logging
    request_count: AtomicU64,
}

impl ModelRouter {
    /// Create a new model router with all three providers
    pub fn new(
        fast_provider: Arc<dyn LlmProvider>,
        voice_provider: Arc<dyn LlmProvider>,
        thinker_provider: Arc<dyn LlmProvider>,
        config: RouterConfig,
    ) -> Self {
        let classifier = TaskClassifier::new(config.clone());

        info!(
            "ModelRouter initialized: Fast={}, Voice={}, Thinker={}, enabled={}",
            fast_provider.name(),
            voice_provider.name(),
            thinker_provider.name(),
            config.enabled
        );

        Self {
            fast_provider,
            voice_provider,
            thinker_provider,
            classifier,
            config,
            stats: RwLock::new(RoutingStats::default()),
            request_count: AtomicU64::new(0),
        }
    }

    /// Create router with default config
    pub fn with_providers(
        fast: Arc<dyn LlmProvider>,
        voice: Arc<dyn LlmProvider>,
        thinker: Arc<dyn LlmProvider>,
    ) -> Self {
        Self::new(fast, voice, thinker, RouterConfig::from_env())
    }

    /// Check if router is enabled
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Get the configuration
    pub fn config(&self) -> &RouterConfig {
        &self.config
    }

    /// Route a task to the appropriate provider
    pub fn route(&self, task: &RoutingTask) -> Arc<dyn LlmProvider> {
        let request_id = self.request_count.fetch_add(1, Ordering::Relaxed);

        // If routing disabled, always use Voice tier (balanced default)
        if !self.config.enabled {
            if self.config.log_routing {
                debug!(
                    "[Route #{}] Routing disabled, using Voice tier",
                    request_id
                );
            }
            return self.voice_provider.clone();
        }

        // Classify the task
        let tier = self.classifier.classify(task);
        let reason = self.classifier.classification_reason(task);

        // Record stats
        if let Ok(mut stats) = self.stats.write() {
            stats.record(tier, task.estimated_tokens);
        }

        // Log routing decision
        if self.config.log_routing {
            debug!(
                "[Route #{}] {} -> {} (reason: {}, tokens: ~{}, files: {})",
                request_id,
                task.tool_name.as_deref().unwrap_or("chat"),
                tier.display_name(),
                reason,
                task.estimated_tokens,
                task.file_count
            );
        }

        // Return the appropriate provider
        self.get_provider(tier)
    }

    /// Get provider for a specific tier
    pub fn get_provider(&self, tier: ModelTier) -> Arc<dyn LlmProvider> {
        match tier {
            ModelTier::Fast => self.fast_provider.clone(),
            ModelTier::Voice => self.voice_provider.clone(),
            ModelTier::Thinker => self.thinker_provider.clone(),
        }
    }

    /// Get provider for user-facing chat (always Voice tier)
    pub fn voice(&self) -> Arc<dyn LlmProvider> {
        self.voice_provider.clone()
    }

    /// Get provider for simple/fast operations
    pub fn fast(&self) -> Arc<dyn LlmProvider> {
        self.fast_provider.clone()
    }

    /// Get provider for complex reasoning
    pub fn thinker(&self) -> Arc<dyn LlmProvider> {
        self.thinker_provider.clone()
    }

    /// Get routing statistics
    pub fn stats(&self) -> RoutingStats {
        self.stats.read().map(|s| s.clone()).unwrap_or_default()
    }

    /// Reset routing statistics
    pub fn reset_stats(&self) {
        if let Ok(mut stats) = self.stats.write() {
            *stats = RoutingStats::default();
        }
    }

    /// Get a summary of routing activity
    pub fn summary(&self) -> String {
        let stats = self.stats();
        format!(
            "Routing: {} total ({:.1}% fast, {:.1}% voice, {:.1}% thinker), est. savings: ${:.2}",
            stats.total_requests(),
            stats.fast_percentage(),
            if stats.total_requests() > 0 {
                (stats.voice_requests as f64 / stats.total_requests() as f64) * 100.0
            } else {
                0.0
            },
            if stats.total_requests() > 0 {
                (stats.thinker_requests as f64 / stats.total_requests() as f64) * 100.0
            } else {
                0.0
            },
            stats.estimated_savings_usd
        )
    }

    /// Route with fallback on failure
    ///
    /// If the primary tier fails and fallback is enabled, try the next tier up.
    /// Fast -> Voice -> Thinker
    pub async fn route_with_fallback<F, T>(&self, task: &RoutingTask, operation: F) -> Result<T>
    where
        F: Fn(Arc<dyn LlmProvider>) -> futures::future::BoxFuture<'static, Result<T>>,
    {
        let primary_tier = self.classifier.classify(task);
        let provider = self.get_provider(primary_tier);

        // Try primary provider
        match operation(provider.clone()).await {
            Ok(result) => Ok(result),
            Err(e) => {
                if !self.config.enable_fallback {
                    return Err(e);
                }

                // Try fallback: Fast -> Voice -> Thinker
                let fallback_tier = match primary_tier {
                    ModelTier::Fast => Some(ModelTier::Voice),
                    ModelTier::Voice => Some(ModelTier::Thinker),
                    ModelTier::Thinker => None,
                };

                if let Some(tier) = fallback_tier {
                    warn!(
                        "Primary {} failed ({}), falling back to {}",
                        primary_tier.display_name(),
                        e,
                        tier.display_name()
                    );

                    let fallback_provider = self.get_provider(tier);
                    operation(fallback_provider).await
                } else {
                    Err(e)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::provider::{Message, Response, TokenUsage, ToolContext, ToolResponse};
    use async_trait::async_trait;
    use serde_json::Value;
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

        async fn chat(&self, _messages: Vec<Message>, _system: String) -> Result<Response> {
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
        ) -> Result<ToolResponse> {
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

    fn test_router() -> ModelRouter {
        let fast = Arc::new(MockProvider { name: "fast-mock" }) as Arc<dyn LlmProvider>;
        let voice = Arc::new(MockProvider { name: "voice-mock" }) as Arc<dyn LlmProvider>;
        let thinker = Arc::new(MockProvider { name: "thinker-mock" }) as Arc<dyn LlmProvider>;

        ModelRouter::new(fast, voice, thinker, RouterConfig::default())
    }

    #[test]
    fn test_router_routes_fast_tools() {
        let router = test_router();

        let task = RoutingTask::from_tool("list_project_files");
        let provider = router.route(&task);

        assert_eq!(provider.name(), "fast-mock");
    }

    #[test]
    fn test_router_routes_chat_to_voice() {
        let router = test_router();

        let task = RoutingTask::user_chat();
        let provider = router.route(&task);

        assert_eq!(provider.name(), "voice-mock");
    }

    #[test]
    fn test_router_routes_complex_to_thinker() {
        let router = test_router();

        let task = RoutingTask::new().with_operation("architecture");
        let provider = router.route(&task);

        assert_eq!(provider.name(), "thinker-mock");
    }

    #[test]
    fn test_router_stats() {
        let router = test_router();

        // Route several tasks
        router.route(&RoutingTask::from_tool("list_project_files"));
        router.route(&RoutingTask::from_tool("search_codebase"));
        router.route(&RoutingTask::user_chat());
        router.route(&RoutingTask::new().with_operation("architecture"));

        let stats = router.stats();
        assert_eq!(stats.fast_requests, 2);
        assert_eq!(stats.voice_requests, 1);
        assert_eq!(stats.thinker_requests, 1);
        assert_eq!(stats.total_requests(), 4);
    }

    #[test]
    fn test_router_disabled() {
        let fast = Arc::new(MockProvider { name: "fast-mock" }) as Arc<dyn LlmProvider>;
        let voice = Arc::new(MockProvider { name: "voice-mock" }) as Arc<dyn LlmProvider>;
        let thinker = Arc::new(MockProvider { name: "thinker-mock" }) as Arc<dyn LlmProvider>;

        let mut config = RouterConfig::default();
        config.enabled = false;

        let router = ModelRouter::new(fast, voice, thinker, config);

        // Even fast tools should go to Voice when disabled
        let task = RoutingTask::from_tool("list_project_files");
        let provider = router.route(&task);

        assert_eq!(provider.name(), "voice-mock");
    }

    #[test]
    fn test_explicit_tier_methods() {
        let router = test_router();

        assert_eq!(router.fast().name(), "fast-mock");
        assert_eq!(router.voice().name(), "voice-mock");
        assert_eq!(router.thinker().name(), "thinker-mock");
    }
}
