// crates/mira-server/src/llm/factory.rs
// Provider factory for managing multiple LLM clients

use crate::config::{ApiKeys, MiraConfig};
use crate::db::pool::DatabasePool;
use crate::llm::deepseek::DeepSeekClient;
use crate::llm::gemini::GeminiClient;
use crate::llm::provider::{LlmClient, Provider};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{info, warn};

/// Factory for creating and managing LLM provider clients
pub struct ProviderFactory {
    clients: HashMap<Provider, Arc<dyn LlmClient>>,
    default_provider: Option<Provider>,
    background_provider: Option<Provider>,
    fallback_order: Vec<Provider>,
    // Store API keys to create custom clients on demand
    deepseek_key: Option<String>,
    gemini_key: Option<String>,
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

        // Initialize Gemini client
        if let Some(ref key) = api_keys.gemini {
            info!("Gemini client initialized");
            clients.insert(Provider::Gemini, Arc::new(GeminiClient::new(key.clone())));
        }

        // Log available providers
        let available: Vec<_> = clients.keys().map(|p| p.to_string()).collect();
        info!(providers = ?available, "LLM providers available");

        // Default fallback order: DeepSeek -> Gemini
        let fallback_order = vec![Provider::DeepSeek, Provider::Gemini];

        Self {
            clients,
            default_provider,
            background_provider,
            fallback_order,
            deepseek_key: api_keys.deepseek,
            gemini_key: api_keys.gemini,
        }
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
        if let Some(ref provider) = self.default_provider {
            if let Some(client) = self.clients.get(provider) {
                return Some(client.clone());
            }
        }

        // Fall back through the chain
        for provider in &self.fallback_order {
            if let Some(client) = self.clients.get(provider) {
                return Some(client.clone());
            }
        }

        None
    }

    /// Get a client for a specific expert role (async to avoid blocking on DB)
    /// Priority: role config -> global default -> fallback chain
    pub async fn client_for_role(
        &self,
        role: &str,
        pool: &Arc<DatabasePool>,
    ) -> Result<Arc<dyn LlmClient>, String> {
        // 1. Check role-specific configuration in database (async!)
        if let Ok(config) = pool.get_expert_config(role).await {
            // If a specific model is configured, try to create a client for it
            if let Some(model) = config.model {
                let client_opt: Option<Arc<dyn LlmClient>> = match config.provider {
                    Provider::DeepSeek => self.deepseek_key.as_ref().map(|k| {
                        Arc::new(DeepSeekClient::with_model(k.clone(), model)) as Arc<dyn LlmClient>
                    }),
                    Provider::Gemini => self.gemini_key.as_ref().map(|k| {
                        Arc::new(GeminiClient::with_model(k.clone(), model)) as Arc<dyn LlmClient>
                    }),
                    _ => None,
                };

                if let Some(client) = client_opt {
                    info!(
                        role = role,
                        provider = %config.provider,
                        model = %client.model_name(),
                        "Using configured provider and model for role"
                    );
                    return Ok(client);
                }
            }

            // Fallback to default client for that provider
            if let Some(client) = self.clients.get(&config.provider) {
                info!(
                    role = role,
                    provider = %config.provider,
                    "Using configured provider for role (default model)"
                );
                return Ok(client.clone());
            } else {
                warn!(
                    role = role,
                    provider = %config.provider,
                    "Configured provider not available, falling back"
                );
            }
        }

        // 2. Try global default if set
        if let Some(ref default) = self.default_provider {
            if let Some(client) = self.clients.get(default) {
                info!(
                    role = role,
                    provider = %default,
                    "Using global default provider"
                );
                return Ok(client.clone());
            }
        }

        // 3. Fall back through the chain
        for provider in &self.fallback_order {
            if let Some(client) = self.clients.get(provider) {
                info!(
                    role = role,
                    provider = %provider,
                    "Using fallback provider"
                );
                return Ok(client.clone());
            }
        }

        Err("No LLM providers available. Set DEEPSEEK_API_KEY or GEMINI_API_KEY.".into())
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

    /// Check if any providers are available
    pub fn has_providers(&self) -> bool {
        !self.clients.is_empty()
    }

    /// Get chat and reasoner clients for expert consultation.
    /// Currently returns (primary, None) — single client for everything.
    /// The agentic tool loop strips reasoning_content from intermediate messages
    /// (see execution.rs, debate.rs) to prevent unbounded memory growth,
    /// so deepseek-reasoner is safe to use for the full loop.
    pub async fn client_for_role_dual_mode(
        &self,
        role: &str,
        pool: &Arc<DatabasePool>,
    ) -> Result<(Arc<dyn LlmClient>, Option<Arc<dyn LlmClient>>), String> {
        let primary = self.client_for_role(role, pool).await?;
        Ok((primary, None))
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
            fallback_order: vec![Provider::DeepSeek, Provider::Gemini],
            deepseek_key: None,
            gemini_key: None,
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
        assert!(!factory.is_available(Provider::Gemini));
    }

    #[test]
    fn test_empty_factory_get_provider_none() {
        let factory = empty_factory();
        assert!(factory.get_provider(Provider::DeepSeek).is_none());
        assert!(factory.get_provider(Provider::Gemini).is_none());
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
            fallback_order: vec![Provider::DeepSeek, Provider::Gemini],
            deepseek_key: None,
            gemini_key: None,
        };
        assert_eq!(factory.default_provider(), Some(Provider::DeepSeek));
    }

    #[test]
    fn test_fallback_order() {
        let factory = empty_factory();
        assert_eq!(factory.fallback_order.len(), 2);
        assert_eq!(factory.fallback_order[0], Provider::DeepSeek);
        assert_eq!(factory.fallback_order[1], Provider::Gemini);
    }
}
