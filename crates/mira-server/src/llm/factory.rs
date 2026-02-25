// crates/mira-server/src/llm/factory.rs
// Provider factory for managing multiple LLM clients

use crate::config::{ApiKeys, MiraConfig};
use crate::llm::circuit_breaker::CircuitBreaker;
use crate::llm::deepseek::DeepSeekClient;
use crate::llm::ollama::OllamaClient;
use crate::llm::provider::{LlmClient, Provider};
use rmcp::service::{Peer, RoleServer};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

/// Factory for creating and managing LLM provider clients
///
/// Note: Background LLM processing is disabled. All background tasks
/// (pondering, summaries, briefings, diff analysis, code health) use
/// local heuristic fallbacks. LLM clients are retained for embeddings
/// and any future optional use.
pub struct ProviderFactory {
    clients: HashMap<Provider, Arc<dyn LlmClient>>,
    default_provider: Option<Provider>,
    // MCP sampling peer — last-resort fallback when no API keys are configured
    sampling_peer: Option<Arc<RwLock<Option<Peer<RoleServer>>>>>,
    circuit_breaker: CircuitBreaker,
}

impl ProviderFactory {
    /// Create a new factory, initializing clients from environment variables
    pub fn new() -> Self {
        Self::from_api_keys(ApiKeys::from_env())
    }

    /// Create a factory from pre-loaded API keys (avoids duplicate env var reads)
    pub fn from_api_keys(api_keys: ApiKeys) -> Self {
        let mut clients: HashMap<Provider, Arc<dyn LlmClient>> = HashMap::new();

        // Load config file for provider preferences
        let config = MiraConfig::load();

        // Config file takes precedence, env var is backwards-compat fallback
        let default_provider = config.default_provider().or_else(|| {
            std::env::var("DEFAULT_LLM_PROVIDER")
                .ok()
                .and_then(|s| Provider::from_str(&s))
        });

        if let Some(ref p) = default_provider {
            info!(provider = %p, "Default LLM provider configured");
        }

        // Initialize DeepSeek client
        if let Some(ref key) = api_keys.deepseek {
            info!("DeepSeek client initialized");
            clients.insert(
                Provider::DeepSeek,
                Arc::new(DeepSeekClient::new(key.clone())),
            );
        }

        // Initialize Ollama client (local LLM, no API key — just a host URL)
        if let Some(ref host) = api_keys.ollama {
            let model = std::env::var("OLLAMA_MODEL")
                .ok()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| Provider::Ollama.default_model().to_string());
            info!(host = %host, model = %model, "Ollama client initialized");
            clients.insert(
                Provider::Ollama,
                Arc::new(OllamaClient::with_model(host.clone(), model)),
            );
        }

        // Log available providers
        let available: Vec<_> = clients.keys().map(|p| p.to_string()).collect();
        info!(providers = ?available, "LLM providers available");

        Self {
            clients,
            default_provider,
            sampling_peer: None,
            circuit_breaker: CircuitBreaker::new(),
        }
    }

    /// Set the MCP sampling peer for zero-key LLM fallback.
    /// Called once the peer is captured from the first tool call.
    pub fn set_sampling_peer(&mut self, peer: Arc<RwLock<Option<Peer<RoleServer>>>>) {
        self.sampling_peer = Some(peer);
    }

    /// Get a client for background tasks (summaries, briefings, capabilities, etc.)
    ///
    /// Always returns None — background tasks use local heuristic fallbacks instead
    /// of external LLM calls. All background consumers (pondering, summaries, briefings,
    /// diff analysis, code health, etc.) already handle None gracefully with
    /// heuristic/template outputs. This eliminates external API costs and latency
    /// for background processing while providing structured facts to Claude.
    pub fn client_for_background(&self) -> Option<Arc<dyn LlmClient>> {
        None
    }

    /// Record a successful LLM call — resets the provider's circuit breaker.
    pub fn record_success(&self, provider: Provider) {
        self.circuit_breaker.record_success(provider);
    }

    /// Record a failed LLM call — may trip the provider's circuit breaker.
    pub fn record_failure(&self, provider: Provider) {
        self.circuit_breaker.record_failure(provider);
    }

    /// Get a reference to the circuit breaker (for testing or diagnostics).
    pub fn circuit_breaker(&self) -> &CircuitBreaker {
        &self.circuit_breaker
    }

    /// Get a specific provider client (if available)
    pub fn get_provider(&self, provider: Provider) -> Option<Arc<dyn LlmClient>> {
        self.clients.get(&provider).cloned()
    }

    /// List all available providers
    pub fn available_providers(&self) -> Vec<Provider> {
        self.clients.keys().copied().collect()
    }

    /// Check if a specific provider is available
    pub fn is_available(&self, provider: Provider) -> bool {
        self.clients.contains_key(&provider)
    }

    /// Get the default provider (if configured)
    pub fn default_provider(&self) -> Option<Provider> {
        self.default_provider
    }

    /// Check if any API-key-based LLM providers are available.
    /// Does NOT count MCP sampling peer — use this to gate background work
    /// and other operations that need a dedicated LLM client.
    pub fn has_providers(&self) -> bool {
        !self.clients.is_empty()
    }

    /// Check if any LLM capability exists (API keys OR MCP sampling fallback).
    pub fn has_any_capability(&self) -> bool {
        !self.clients.is_empty() || self.sampling_peer.is_some()
    }
}

impl Default for ProviderFactory {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create an empty factory for testing (no env vars)
    fn empty_factory() -> ProviderFactory {
        ProviderFactory {
            clients: HashMap::new(),
            default_provider: None,
            sampling_peer: None,
            circuit_breaker: CircuitBreaker::new(),
        }
    }

    #[test]
    fn test_empty_factory_has_no_providers() {
        let factory = empty_factory();
        assert!(!factory.has_providers());
        assert!(!factory.has_any_capability());
        assert!(factory.available_providers().is_empty());
    }

    #[test]
    fn test_empty_factory_is_available_false() {
        let factory = empty_factory();
        assert!(!factory.is_available(Provider::DeepSeek));
    }

    #[test]
    fn test_empty_factory_get_provider_none() {
        let factory = empty_factory();
        assert!(factory.get_provider(Provider::DeepSeek).is_none());
    }

    #[test]
    fn test_empty_factory_default_provider_none() {
        let factory = empty_factory();
        assert!(factory.default_provider().is_none());
    }

    #[test]
    fn test_factory_with_default_provider() {
        let mut factory = empty_factory();
        factory.default_provider = Some(Provider::DeepSeek);
        assert_eq!(factory.default_provider(), Some(Provider::DeepSeek));
    }

    // ========================================================================
    // client_for_background (always None — background uses heuristics)
    // ========================================================================

    #[test]
    fn test_client_for_background_always_returns_none() {
        let factory = empty_factory();
        assert!(factory.client_for_background().is_none());
    }

    #[test]
    fn test_client_for_background_none_even_with_providers() {
        let mut factory = empty_factory();
        factory.clients.insert(
            Provider::DeepSeek,
            Arc::new(DeepSeekClient::new("test-key".into())),
        );
        factory.clients.insert(
            Provider::Ollama,
            Arc::new(OllamaClient::new("http://localhost:11434".into())),
        );
        // Background always returns None regardless of available providers
        assert!(factory.client_for_background().is_none());
    }

    // ========================================================================
    // Circuit breaker (retained for non-background uses)
    // ========================================================================

    #[test]
    fn test_record_success_on_tripped_breaker_does_not_panic() {
        let mut factory = empty_factory();
        factory.clients.insert(
            Provider::DeepSeek,
            Arc::new(DeepSeekClient::new("test-key".into())),
        );
        // Trip circuit breaker
        factory.record_failure(Provider::DeepSeek);
        factory.record_failure(Provider::DeepSeek);
        factory.record_failure(Provider::DeepSeek);
        assert!(!factory.circuit_breaker().is_available(Provider::DeepSeek));
        // An open breaker requires cooldown before reset — record_success
        // on a tripped breaker is a no-op, but must not panic.
        factory.record_success(Provider::DeepSeek);
    }

    // ========================================================================
    // has_any_capability
    // ========================================================================

    #[test]
    fn test_has_any_capability_with_sampling_peer() {
        let mut factory = empty_factory();
        assert!(!factory.has_any_capability());
        factory.set_sampling_peer(Arc::new(RwLock::new(None)));
        assert!(factory.has_any_capability());
    }

    #[test]
    fn test_has_providers_vs_has_any_capability() {
        let mut factory = empty_factory();
        factory.set_sampling_peer(Arc::new(RwLock::new(None)));
        // has_providers checks API-key clients only
        assert!(!factory.has_providers());
        // has_any_capability includes sampling peer
        assert!(factory.has_any_capability());
    }
}
