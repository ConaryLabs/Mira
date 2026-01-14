// crates/mira-server/src/llm/factory.rs
// Provider factory for managing multiple LLM clients

use crate::db::Database;
use crate::llm::deepseek::DeepSeekClient;
use crate::llm::gemini::GeminiClient;
use crate::llm::openai::OpenAiClient;
use crate::llm::provider::{LlmClient, Provider};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{info, warn};

/// Factory for creating and managing LLM provider clients
pub struct ProviderFactory {
    clients: HashMap<Provider, Arc<dyn LlmClient>>,
    default_provider: Option<Provider>,
    fallback_order: Vec<Provider>,
}

impl ProviderFactory {
    /// Create a new factory, initializing clients from environment variables
    pub fn new() -> Self {
        let mut clients: HashMap<Provider, Arc<dyn LlmClient>> = HashMap::new();

        // Check for global default provider
        let default_provider = std::env::var("DEFAULT_LLM_PROVIDER")
            .ok()
            .and_then(|s| Provider::from_str(&s));

        if let Some(ref p) = default_provider {
            info!(provider = %p, "Global default LLM provider configured");
        }

        // Initialize DeepSeek client
        if let Ok(key) = std::env::var("DEEPSEEK_API_KEY") {
            if !key.trim().is_empty() {
                info!("DeepSeek client initialized");
                clients.insert(Provider::DeepSeek, Arc::new(DeepSeekClient::new(key)));
            }
        }

        // Initialize OpenAI client
        if let Ok(key) = std::env::var("OPENAI_API_KEY") {
            if !key.trim().is_empty() {
                info!("OpenAI client initialized");
                clients.insert(Provider::OpenAi, Arc::new(OpenAiClient::new(key)));
            }
        }

        // Initialize Gemini client
        if let Ok(key) = std::env::var("GEMINI_API_KEY") {
            if !key.trim().is_empty() {
                info!("Gemini client initialized");
                clients.insert(Provider::Gemini, Arc::new(GeminiClient::new(key)));
            }
        }

        // Log available providers
        let available: Vec<_> = clients.keys().map(|p| p.to_string()).collect();
        info!(providers = ?available, "LLM providers available");

        // Default fallback order: DeepSeek -> OpenAI -> Gemini
        let fallback_order = vec![Provider::DeepSeek, Provider::OpenAi, Provider::Gemini];

        Self {
            clients,
            default_provider,
            fallback_order,
        }
    }

    /// Get a client for a specific expert role
    /// Priority: role config -> global default -> fallback chain
    pub fn client_for_role(
        &self,
        role: &str,
        db: &Database,
    ) -> Result<Arc<dyn LlmClient>, String> {
        // 1. Check role-specific configuration in database
        if let Ok(config) = db.get_expert_config(role) {
            if let Some(client) = self.clients.get(&config.provider) {
                info!(
                    role = role,
                    provider = %config.provider,
                    "Using configured provider for role"
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

        Err("No LLM providers available. Set DEEPSEEK_API_KEY, OPENAI_API_KEY, or GEMINI_API_KEY.".into())
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
}

impl Default for ProviderFactory {
    fn default() -> Self {
        Self::new()
    }
}
