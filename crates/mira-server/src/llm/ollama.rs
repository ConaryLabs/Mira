// crates/mira-server/src/llm/ollama.rs
// Ollama API client via OpenAI-compatible endpoint (local LLM)

use crate::llm::http_client::LlmHttpClient;
use crate::llm::openai_compat::{CompatChatConfig, execute_openai_compat_chat};
use crate::llm::provider::{LlmClient, Provider};
use crate::llm::{ChatResult, Message, Tool};
use anyhow::Result;
use async_trait::async_trait;
use std::time::Duration;
use tracing::instrument;

/// Normalize Ollama base URL by stripping trailing slashes and /v1 suffix
fn normalize_base_url(url: &str) -> String {
    let mut url = url.trim_end_matches('/').to_string();
    if url.ends_with("/v1") {
        url.truncate(url.len() - 3);
    }
    url
}

/// Check if a URL points to a local address (localhost, 127.0.0.1, [::1])
fn is_local_url(url: &str) -> bool {
    match url::Url::parse(url) {
        Ok(parsed) => match parsed.host() {
            Some(url::Host::Domain(d)) => d == "localhost",
            Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
            Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
            None => true, // No host (e.g. unix socket) — treat as local
        },
        Err(_) => true, // Can't parse — don't warn on malformed URLs
    }
}

/// Ollama API client (OpenAI-compatible endpoint, no auth required)
pub struct OllamaClient {
    base_url: String,
    model: String,
    http: LlmHttpClient,
}

impl OllamaClient {
    /// Create a new Ollama client with default model (llama3.3)
    pub fn new(base_url: String) -> Self {
        Self::with_model(base_url, "llama3.3".into())
    }

    /// Create a new Ollama client with custom model
    pub fn with_model(base_url: String, model: String) -> Self {
        let http = LlmHttpClient::new(Duration::from_secs(300), Duration::from_secs(30));
        let normalized = normalize_base_url(&base_url);

        if !is_local_url(&normalized) {
            tracing::warn!(
                "OLLAMA_HOST points to non-local address '{}'. For security, consider using localhost.",
                normalized
            );
        }

        Self {
            base_url: normalized,
            model,
            http,
        }
    }

    /// Chat using Ollama model (non-streaming, OpenAI-compatible)
    #[instrument(skip(self, messages, tools), fields(request_id, model = %self.model, message_count = messages.len()))]
    pub async fn chat(
        &self,
        messages: Vec<Message>,
        tools: Option<Vec<Tool>>,
    ) -> Result<ChatResult> {
        let config = CompatChatConfig {
            provider_name: "Ollama",
            model: self.model.clone(),
            supports_budget: self.supports_context_budget(),
            max_tokens: None,
        };

        let url = format!("{}/v1/chat/completions", self.base_url);

        execute_openai_compat_chat(config, messages, tools, |req_id, body| async move {
            // No auth header needed for local Ollama
            self.http
                .execute_request_with_retry(&req_id, body, |client, body| {
                    client
                        .post(&url)
                        .header("Content-Type", "application/json")
                        .body(body)
                })
                .await
        })
        .await
    }
}

#[async_trait]
impl LlmClient for OllamaClient {
    fn provider_type(&self) -> Provider {
        Provider::Ollama
    }

    fn model_name(&self) -> String {
        self.model.clone()
    }

    /// Conservative budget for local models: 32K tokens
    fn context_budget(&self) -> u64 {
        32_000
    }

    async fn chat(&self, messages: Vec<Message>, tools: Option<Vec<Tool>>) -> Result<ChatResult> {
        self.chat(messages, tools).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ollama_client_creation() {
        let client = OllamaClient::new("http://localhost:11434".into());
        assert_eq!(client.model, "llama3.3");
        assert_eq!(client.base_url, "http://localhost:11434");
        assert_eq!(client.provider_type(), Provider::Ollama);
    }

    #[test]
    fn test_ollama_client_custom_model() {
        let client = OllamaClient::with_model("http://myhost:11434".into(), "mistral".into());
        assert_eq!(client.model, "mistral");
        assert_eq!(client.base_url, "http://myhost:11434");
        assert_eq!(client.model_name(), "mistral");
    }

    #[test]
    fn test_ollama_context_budget() {
        let client = OllamaClient::new("http://localhost:11434".into());
        assert_eq!(client.context_budget(), 32_000);
        assert!(client.supports_context_budget());
    }

    #[test]
    fn test_is_local_url() {
        assert!(is_local_url("http://localhost:11434"));
        assert!(is_local_url("http://127.0.0.1:11434"));
        assert!(is_local_url("http://[::1]:11434"));
        assert!(!is_local_url("http://192.168.1.100:11434"));
        assert!(!is_local_url("http://myhost:11434"));
        assert!(!is_local_url("https://ollama.example.com:11434"));
    }

    #[test]
    fn test_url_normalization() {
        let client = OllamaClient::new("http://localhost:11434/v1".into());
        assert_eq!(client.base_url, "http://localhost:11434");

        let client = OllamaClient::new("http://localhost:11434/v1/".into());
        assert_eq!(client.base_url, "http://localhost:11434");

        let client = OllamaClient::new("http://localhost:11434/".into());
        assert_eq!(client.base_url, "http://localhost:11434");

        let client = OllamaClient::new("http://localhost:11434".into());
        assert_eq!(client.base_url, "http://localhost:11434");
    }
}
