// crates/mira-server/src/llm/factory.rs
// Provider factory for managing multiple LLM clients

use crate::config::{ApiKeys, MiraConfig};
use crate::llm::deepseek::DeepSeekClient;
use crate::llm::ollama::OllamaClient;
use crate::llm::provider::{LlmClient, Provider};
use crate::llm::zhipu::ZhipuClient;
use rmcp::service::{Peer, RoleServer};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

/// Factory for creating and managing LLM provider clients
pub struct ProviderFactory {
    clients: HashMap<Provider, Arc<dyn LlmClient>>,
    default_provider: Option<Provider>,
    background_provider: Option<Provider>,
    background_fallback_order: Vec<Provider>,
    // MCP sampling peer — last-resort fallback when no API keys are configured
    sampling_peer: Option<Arc<RwLock<Option<Peer<RoleServer>>>>>,
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

        // Check for expert provider: config file first, then env var
        let default_provider = config.expert_provider().or_else(|| {
            std::env::var("DEFAULT_LLM_PROVIDER")
                .ok()
                .and_then(|s| Provider::from_str(&s))
        });

        // Check for background provider from config
        let background_provider = config.background_provider();

        if let Some(ref p) = default_provider {
            info!(provider = %p, "Default LLM provider configured");
        }
        if let Some(ref p) = background_provider {
            info!(provider = %p, "Background LLM provider configured");
        }

        // Initialize DeepSeek client
        if let Some(ref key) = api_keys.deepseek {
            info!("DeepSeek client initialized");
            clients.insert(
                Provider::DeepSeek,
                Arc::new(DeepSeekClient::new(key.clone())),
            );
        }

        // Initialize Zhipu GLM client
        if let Some(ref key) = api_keys.zhipu {
            info!("Zhipu GLM client initialized");
            clients.insert(Provider::Zhipu, Arc::new(ZhipuClient::new(key.clone())));
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

        let background_fallback_order = vec![Provider::DeepSeek, Provider::Zhipu, Provider::Ollama];

        Self {
            clients,
            default_provider,
            background_provider,
            background_fallback_order,
            sampling_peer: None,
        }
    }

    /// Set the MCP sampling peer for zero-key expert fallback.
    /// Called once the peer is captured from the first tool call.
    pub fn set_sampling_peer(&mut self, peer: Arc<RwLock<Option<Peer<RoleServer>>>>) {
        self.sampling_peer = Some(peer);
    }

    /// Get a client for background tasks (summaries, briefings, capabilities, etc.)
    /// Priority: background_provider config -> default_provider -> fallback chain
    pub fn client_for_background(&self) -> Option<Arc<dyn LlmClient>> {
        // Try background provider first
        if let Some(ref provider) = self.background_provider {
            if let Some(client) = self.clients.get(provider) {
                return Some(client.clone());
            }
            warn!(provider = %provider, "Configured background provider not available");
        }

        // Fall back to default provider
        if let Some(ref provider) = self.default_provider
            && let Some(client) = self.clients.get(provider)
        {
            return Some(client.clone());
        }

        // Fall back through the chain (includes Ollama for background tasks)
        for provider in &self.background_fallback_order {
            if let Some(client) = self.clients.get(provider) {
                return Some(client.clone());
            }
        }

        None
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

    /// Check if any providers are available (including sampling fallback)
    pub fn has_providers(&self) -> bool {
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
            background_provider: None,

            background_fallback_order: vec![Provider::DeepSeek, Provider::Zhipu, Provider::Ollama],
            sampling_peer: None,
        }
    }

    #[test]
    fn test_empty_factory_has_no_providers() {
        let factory = empty_factory();
        assert!(!factory.has_providers());
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
        let factory = ProviderFactory {
            clients: HashMap::new(),
            default_provider: Some(Provider::DeepSeek),
            background_provider: None,

            background_fallback_order: vec![Provider::DeepSeek, Provider::Zhipu, Provider::Ollama],
            sampling_peer: None,
        };
        assert_eq!(factory.default_provider(), Some(Provider::DeepSeek));
    }

    #[test]
    fn test_background_fallback_order() {
        let factory = empty_factory();
        // Background fallback includes Ollama
        assert_eq!(factory.background_fallback_order.len(), 3);
        assert_eq!(factory.background_fallback_order[0], Provider::DeepSeek);
        assert_eq!(factory.background_fallback_order[1], Provider::Zhipu);
        assert_eq!(factory.background_fallback_order[2], Provider::Ollama);
    }

}
