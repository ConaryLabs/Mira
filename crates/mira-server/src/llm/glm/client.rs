// crates/mira-server/src/llm/glm/client.rs
// GLM 4.7 (Z.AI) API client with thinking mode support

use crate::llm::http_client::LlmHttpClient;
use crate::llm::openai_compat::{parse_chat_response, ChatRequest};
use crate::llm::provider::{LlmClient, Provider};
use crate::llm::truncate_messages_to_budget;
use crate::llm::{ChatResult, Message, Tool};
use anyhow::Result;
use async_trait::async_trait;
use reqwest::Client;
use std::time::{Duration, Instant};
use tracing::{debug, info, instrument, Span};
use uuid::Uuid;

const GLM_API_URL: &str = "https://api.z.ai/api/paas/v4/chat/completions";
const MAX_OUTPUT_TOKENS: u32 = 128_000;
const DEFAULT_TEMPERATURE: f32 = 0.01; // GLM requires (0,1), can't use 0
const THINKING_BUDGET_TOKENS: u32 = 8192;

/// GLM 4.7 API client
pub struct GlmClient {
    api_key: String,
    model: String,
    http: LlmHttpClient,
    thinking_enabled: bool,
}

impl GlmClient {
    /// Create a new GLM client with default model (glm-4.7)
    pub fn new(api_key: String) -> Self {
        Self::with_model(api_key, "glm-4.7".into())
    }

    /// Create a new GLM client with custom model
    pub fn with_model(api_key: String, model: String) -> Self {
        let http = LlmHttpClient::new(
            Duration::from_secs(300),
            Duration::from_secs(30),
        );
        Self {
            api_key,
            model,
            http,
            thinking_enabled: true, // Enable by default like DeepSeek reasoner
        }
    }

    /// Create a new GLM client with a shared HTTP client
    pub fn with_http_client(api_key: String, model: String, client: Client) -> Self {
        Self {
            api_key,
            model,
            http: LlmHttpClient::from_client(client),
            thinking_enabled: true,
        }
    }

    /// Enable or disable thinking mode
    pub fn with_thinking(mut self, enabled: bool) -> Self {
        self.thinking_enabled = enabled;
        self
    }
}

#[async_trait]
impl LlmClient for GlmClient {
    fn provider_type(&self) -> Provider {
        Provider::Glm
    }

    fn model_name(&self) -> String {
        self.model.clone()
    }

    /// GLM has 200K context, benefits from budget management
    fn supports_context_budget(&self) -> bool {
        true
    }

    #[instrument(skip(self, messages, tools), fields(request_id, model = %self.model, message_count = messages.len()))]
    async fn chat(&self, messages: Vec<Message>, tools: Option<Vec<Tool>>) -> Result<ChatResult> {
        let request_id = Uuid::new_v4().to_string();
        let start_time = Instant::now();

        Span::current().record("request_id", &request_id);

        // Apply budget-aware truncation
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
            thinking = self.thinking_enabled,
            "Starting GLM chat request"
        );

        // Build request using shared ChatRequest
        let mut request = ChatRequest::new(&self.model, messages)
            .with_tools(tools)
            .with_max_tokens(MAX_OUTPUT_TOKENS)
            .with_temperature(DEFAULT_TEMPERATURE);

        if self.thinking_enabled {
            request = request.with_thinking(true, THINKING_BUDGET_TOKENS);
        }

        let body = serde_json::to_string(&request)?;
        debug!(request_id = %request_id, "GLM request: {}", body);

        let response_body = self.http.execute_with_retry(
            &request_id,
            GLM_API_URL,
            &self.api_key,
            body,
        ).await?;

        let duration_ms = start_time.elapsed().as_millis() as u64;

        // Parse response using shared parser
        let result = parse_chat_response(&response_body, request_id.clone(), duration_ms)?;

        // Log usage stats
        if let Some(ref u) = result.usage {
            info!(
                request_id = %request_id,
                prompt_tokens = u.prompt_tokens,
                completion_tokens = u.completion_tokens,
                "GLM usage stats"
            );
        }

        // Log tool calls if any
        if let Some(ref tcs) = result.tool_calls {
            info!(
                request_id = %request_id,
                tool_count = tcs.len(),
                tools = ?tcs.iter().map(|tc| &tc.function.name).collect::<Vec<_>>(),
                "GLM requested tool calls"
            );
            for tc in tcs {
                let args: serde_json::Value =
                    serde_json::from_str(&tc.function.arguments).unwrap_or(serde_json::Value::Null);
                debug!(
                    request_id = %request_id,
                    tool = %tc.function.name,
                    call_id = %tc.id,
                    args = %args,
                    "Tool call"
                );
            }
        }

        info!(
            request_id = %request_id,
            duration_ms = duration_ms,
            content_len = result.content.as_ref().map(|c| c.len()).unwrap_or(0),
            reasoning_len = result.reasoning_content.as_ref().map(|r| r.len()).unwrap_or(0),
            tool_calls = result.tool_calls.as_ref().map(|t| t.len()).unwrap_or(0),
            "GLM chat complete"
        );

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_new() {
        let client = GlmClient::new("test-key".to_string());
        assert_eq!(client.model, "glm-4.7");
        assert!(client.thinking_enabled);
    }

    #[test]
    fn test_client_with_model() {
        let client = GlmClient::with_model("test-key".to_string(), "glm-4-plus".to_string());
        assert_eq!(client.model, "glm-4-plus");
    }

    #[test]
    fn test_client_with_thinking() {
        let client = GlmClient::new("test-key".to_string()).with_thinking(false);
        assert!(!client.thinking_enabled);
    }

    #[test]
    fn test_provider_type() {
        let client = GlmClient::new("test-key".to_string());
        assert_eq!(client.provider_type(), Provider::Glm);
    }

    #[test]
    fn test_supports_context_budget() {
        let client = GlmClient::new("test-key".to_string());
        assert!(client.supports_context_budget());
    }
}
