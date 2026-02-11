// crates/mira-server/src/llm/zhipu.rs
// Zhipu GLM-5 API client via coding endpoint

use crate::llm::http_client::LlmHttpClient;
use crate::llm::openai_compat::{ChatRequest, parse_chat_response};
use crate::llm::provider::{LlmClient, Provider};
use crate::llm::truncate_messages_to_default_budget;
use crate::llm::{ChatResult, Message, Tool};
use anyhow::Result;
use async_trait::async_trait;
use std::time::{Duration, Instant};
use tracing::{Span, debug, info, instrument};
use uuid::Uuid;

/// GLM coding endpoint (NOT the general API)
const ZHIPU_CODING_API_URL: &str = "https://api.z.ai/api/coding/paas/v4/chat/completions";

/// Zhipu GLM API client
pub struct ZhipuClient {
    api_key: String,
    model: String,
    http: LlmHttpClient,
}

impl ZhipuClient {
    /// Create a new Zhipu client with default model (GLM-5)
    pub fn new(api_key: String) -> Self {
        Self::with_model(api_key, "glm-5".into())
    }

    /// Create a new Zhipu client with custom model
    pub fn with_model(api_key: String, model: String) -> Self {
        let http = LlmHttpClient::new(Duration::from_secs(300), Duration::from_secs(30));
        Self {
            api_key,
            model,
            http,
        }
    }

    /// Chat using GLM model (non-streaming, OpenAI-compatible)
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
            let messages = truncate_messages_to_default_budget(messages);
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
            "Starting Zhipu GLM chat request"
        );

        // Build request using shared ChatRequest
        let request = ChatRequest::new(&self.model, messages)
            .with_tools(tools)
            .with_max_tokens(131_072);

        let body = serde_json::to_string(&request)?;
        debug!(request_id = %request_id, "Zhipu request: {}", body);

        let response_body = self
            .http
            .execute_with_retry(&request_id, ZHIPU_CODING_API_URL, &self.api_key, body)
            .await?;

        let duration_ms = start_time.elapsed().as_millis() as u64;

        // Parse response using shared parser
        let result = parse_chat_response(&response_body, request_id.clone(), duration_ms)?;

        // Log usage stats
        if let Some(ref u) = result.usage {
            crate::llm::logging::log_usage(&request_id, "Zhipu", u);
        }

        if let Some(ref tcs) = result.tool_calls {
            crate::llm::logging::log_tool_calls(&request_id, "Zhipu", tcs);
        }

        crate::llm::logging::log_completion(
            &request_id,
            "Zhipu",
            duration_ms,
            result.content.as_ref().map(|c| c.len()).unwrap_or(0),
            result
                .reasoning_content
                .as_ref()
                .map(|r| r.len())
                .unwrap_or(0),
            result.tool_calls.as_ref().map(|t| t.len()).unwrap_or(0),
        );

        Ok(result)
    }
}

#[async_trait]
impl LlmClient for ZhipuClient {
    fn provider_type(&self) -> Provider {
        Provider::Zhipu
    }

    fn model_name(&self) -> String {
        self.model.clone()
    }

    /// GLM-5 budget: 170K tokens (85% of 200K context window)
    fn context_budget(&self) -> u64 {
        170_000
    }

    async fn chat(&self, messages: Vec<Message>, tools: Option<Vec<Tool>>) -> Result<ChatResult> {
        self.chat(messages, tools).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zhipu_client_creation() {
        let client = ZhipuClient::new("test-key".into());
        assert_eq!(client.model, "glm-5");
        assert_eq!(client.provider_type(), Provider::Zhipu);
    }

    #[test]
    fn test_zhipu_client_custom_model() {
        let client = ZhipuClient::with_model("test-key".into(), "GLM-4.5".into());
        assert_eq!(client.model, "GLM-4.5");
        assert_eq!(client.model_name(), "GLM-4.5");
    }

    #[test]
    fn test_zhipu_context_budget() {
        let client = ZhipuClient::new("test-key".into());
        assert_eq!(client.context_budget(), 170_000);
        assert!(client.supports_context_budget());
    }
}
