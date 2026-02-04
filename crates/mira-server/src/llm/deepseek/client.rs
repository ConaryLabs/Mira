// crates/mira-server/src/llm/deepseek/client.rs
// DeepSeek API client (non-streaming, uses deepseek-reasoner)

use crate::llm::http_client::LlmHttpClient;
use crate::llm::openai_compat::{ChatRequest, parse_chat_response};
use crate::llm::provider::{LlmClient, Provider};
use crate::llm::truncate_messages_to_budget;
use crate::llm::{ChatResult, Message, Tool};
use anyhow::Result;
use async_trait::async_trait;
use std::time::{Duration, Instant};
use tracing::{Span, debug, info, instrument};
use uuid::Uuid;

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
        let request_id = Uuid::new_v4().to_string();
        let start_time = Instant::now();

        Span::current().record("request_id", &request_id);

        // Apply budget-aware truncation if enabled
        let messages = if self.supports_context_budget() {
            let original_count = messages.len();
            let messages = truncate_messages_to_budget(messages);
            if messages.len() != original_count {
                info!(
                    request_id = %request_id,
                    original_messages = original_count,
                    truncated_messages = messages.len(),
                    "Applied context budget truncation"
                );
            }
            messages
        } else {
            messages
        };

        info!(
            request_id = %request_id,
            message_count = messages.len(),
            tool_count = tools.as_ref().map(|t| t.len()).unwrap_or(0),
            model = %self.model,
            "Starting DeepSeek chat request"
        );

        // Build request using shared ChatRequest
        let max_tokens = Self::max_tokens_for_model(&self.model);
        let request = ChatRequest::new(&self.model, messages)
            .with_tools(tools)
            .with_max_tokens(max_tokens);

        let body = serde_json::to_string(&request)?;
        debug!(request_id = %request_id, "DeepSeek request: {}", body);

        let response_body = self
            .http
            .execute_with_retry(&request_id, DEEPSEEK_API_URL, &self.api_key, body)
            .await?;

        let duration_ms = start_time.elapsed().as_millis() as u64;

        // Parse response using shared parser
        let result = parse_chat_response(&response_body, request_id.clone(), duration_ms)?;

        // Log usage stats with DeepSeek-specific cache metrics
        if let Some(ref u) = result.usage {
            crate::llm::logging::log_usage(&request_id, "DeepSeek", u);

            // DeepSeek-specific: cache hit/miss stats
            let cache_hit_ratio = Self::calculate_cache_hit_ratio(
                u.prompt_cache_hit_tokens,
                u.prompt_cache_miss_tokens,
            );
            if cache_hit_ratio.is_some() {
                info!(
                    request_id = %request_id,
                    cache_hit = ?u.prompt_cache_hit_tokens,
                    cache_miss = ?u.prompt_cache_miss_tokens,
                    cache_hit_ratio = ?cache_hit_ratio.map(|r| format!("{:.1}%", r * 100.0)),
                    "DeepSeek cache stats"
                );
            }
        }

        if let Some(ref tcs) = result.tool_calls {
            crate::llm::logging::log_tool_calls(&request_id, "DeepSeek", tcs);
        }

        crate::llm::logging::log_completion(
            &request_id,
            "DeepSeek",
            duration_ms,
            result.content.as_ref().map(|c| c.len()).unwrap_or(0),
            result.reasoning_content.as_ref().map(|r| r.len()).unwrap_or(0),
            result.tool_calls.as_ref().map(|t| t.len()).unwrap_or(0),
        );

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

    /// DeepSeek needs budget management due to 131k token limit
    fn supports_context_budget(&self) -> bool {
        true
    }

    async fn chat(&self, messages: Vec<Message>, tools: Option<Vec<Tool>>) -> Result<ChatResult> {
        // Delegate to the existing implementation
        self.chat(messages, tools).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_cache_hit_ratio() {
        // Test cases
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
