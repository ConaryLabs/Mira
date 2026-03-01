// crates/mira-server/src/llm/deepseek.rs
// DeepSeek API client (non-streaming, uses deepseek-reasoner)

use crate::llm::http_client::LlmHttpClient;
use crate::llm::openai_compat::{CompatChatConfig, execute_openai_compat_chat};
use crate::llm::provider::{LlmClient, Provider};
use crate::llm::{ChatResult, Message, Tool};
use anyhow::Result;
use async_trait::async_trait;
use std::time::Duration;
use tracing::{info, instrument};

const DEEPSEEK_API_URL: &str = "https://api.deepseek.com/chat/completions";

/// DeepSeek API client
pub struct DeepSeekClient {
    api_key: String,
    model: String,
    http: LlmHttpClient,
}

impl DeepSeekClient {
    /// Create a new DeepSeek client with appropriate timeouts
    pub fn new(api_key: String) -> Self {
        Self::with_model(api_key, "deepseek-reasoner".into())
    }

    /// Create a new DeepSeek client with custom model
    pub fn with_model(api_key: String, model: String) -> Self {
        let http = LlmHttpClient::new(Duration::from_secs(300), Duration::from_secs(30));
        Self {
            api_key,
            model,
            http,
        }
    }

    /// Get model-specific max_tokens limit
    /// - deepseek-chat: 8192 (API limit)
    /// - deepseek-reasoner: 65536 (64k limit for synthesis)
    fn max_tokens_for_model(model: &str) -> u32 {
        if model.contains("reasoner") {
            65536 // Reasoner models support up to 64k output
        } else {
            8192 // Chat models have 8k limit
        }
    }

    /// Calculate cache hit ratio from hit and miss token counts
    fn calculate_cache_hit_ratio(hit: Option<u32>, miss: Option<u32>) -> Option<f64> {
        match (hit, miss) {
            (Some(hit), Some(miss)) if hit + miss > 0 => Some((hit as f64) / ((hit + miss) as f64)),
            _ => None,
        }
    }

    /// Chat using deepseek-reasoner model (non-streaming)
    #[instrument(skip(self, messages, tools), fields(request_id, model = %self.model, message_count = messages.len()))]
    pub async fn chat(
        &self,
        messages: Vec<Message>,
        tools: Option<Vec<Tool>>,
    ) -> Result<ChatResult> {
        let config = CompatChatConfig {
            provider_name: "DeepSeek",
            model: self.model.clone(),
            supports_budget: self.supports_context_budget(),
            max_tokens: Some(Self::max_tokens_for_model(&self.model)),
        };

        let result =
            execute_openai_compat_chat(config, messages, tools, |req_id, body| async move {
                self.http
                    .execute_with_retry(&req_id, DEEPSEEK_API_URL, &self.api_key, body)
                    .await
            })
            .await?;

        // DeepSeek-specific: log usage stats with cache metrics
        if let Some(ref u) = result.usage {
            crate::llm::logging::log_usage(&result.request_id, "DeepSeek", u);

            let cache_hit_ratio = Self::calculate_cache_hit_ratio(
                u.prompt_cache_hit_tokens,
                u.prompt_cache_miss_tokens,
            );
            if cache_hit_ratio.is_some() {
                info!(
                    request_id = %result.request_id,
                    cache_hit = ?u.prompt_cache_hit_tokens,
                    cache_miss = ?u.prompt_cache_miss_tokens,
                    cache_hit_ratio = ?cache_hit_ratio.map(|r| format!("{:.1}%", r * 100.0)),
                    "DeepSeek cache stats"
                );
            }
        }

        Ok(result)
    }
}

#[async_trait]
impl LlmClient for DeepSeekClient {
    fn provider_type(&self) -> Provider {
        Provider::DeepSeek
    }

    fn model_name(&self) -> String {
        self.model.clone()
    }

    /// DeepSeek budget: 110K tokens (85% of 128K context window)
    fn context_budget(&self) -> u64 {
        110_000
    }

    async fn chat(&self, messages: Vec<Message>, tools: Option<Vec<Tool>>) -> Result<ChatResult> {
        // Delegate to the existing implementation
        self.chat(messages, tools).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // Client construction
    // ========================================================================

    #[test]
    fn test_new_uses_reasoner_model() {
        let client = DeepSeekClient::new("test-key".into());
        assert_eq!(client.model, "deepseek-reasoner");
    }

    #[test]
    fn test_with_model_custom() {
        let client = DeepSeekClient::with_model("key".into(), "deepseek-chat".into());
        assert_eq!(client.model, "deepseek-chat");
    }

    #[test]
    fn test_provider_type() {
        let client = DeepSeekClient::new("key".into());
        assert_eq!(client.provider_type(), Provider::DeepSeek);
    }

    #[test]
    fn test_model_name() {
        let client = DeepSeekClient::with_model("key".into(), "deepseek-chat".into());
        assert_eq!(client.model_name(), "deepseek-chat");
    }

    #[test]
    fn test_context_budget() {
        let client = DeepSeekClient::new("key".into());
        assert_eq!(client.context_budget(), 110_000);
        assert!(client.supports_context_budget());
    }

    // ========================================================================
    // max_tokens_for_model
    // ========================================================================

    #[test]
    fn test_max_tokens_reasoner() {
        assert_eq!(
            DeepSeekClient::max_tokens_for_model("deepseek-reasoner"),
            65536
        );
    }

    #[test]
    fn test_max_tokens_chat() {
        assert_eq!(DeepSeekClient::max_tokens_for_model("deepseek-chat"), 8192);
    }

    #[test]
    fn test_max_tokens_unknown_model_defaults_to_chat() {
        assert_eq!(DeepSeekClient::max_tokens_for_model("gpt-4"), 8192);
    }

    #[test]
    fn test_max_tokens_reasoner_substring() {
        // Any model containing "reasoner" gets the high limit
        assert_eq!(
            DeepSeekClient::max_tokens_for_model("my-reasoner-v2"),
            65536
        );
    }

    // ========================================================================
    // calculate_cache_hit_ratio
    // ========================================================================

    #[test]
    fn test_calculate_cache_hit_ratio() {
        assert_eq!(
            DeepSeekClient::calculate_cache_hit_ratio(Some(100), Some(100)),
            Some(0.5)
        );
        assert_eq!(
            DeepSeekClient::calculate_cache_hit_ratio(Some(75), Some(25)),
            Some(0.75)
        );
        assert_eq!(
            DeepSeekClient::calculate_cache_hit_ratio(Some(0), Some(100)),
            Some(0.0)
        );
        assert_eq!(
            DeepSeekClient::calculate_cache_hit_ratio(Some(100), Some(0)),
            Some(1.0)
        );
        assert_eq!(
            DeepSeekClient::calculate_cache_hit_ratio(Some(0), Some(0)),
            None
        );
        assert_eq!(
            DeepSeekClient::calculate_cache_hit_ratio(None, Some(100)),
            None
        );
        assert_eq!(
            DeepSeekClient::calculate_cache_hit_ratio(Some(100), None),
            None
        );
        assert_eq!(DeepSeekClient::calculate_cache_hit_ratio(None, None), None);
    }
}
