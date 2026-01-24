// crates/mira-server/src/llm/deepseek/client.rs
// DeepSeek API client (non-streaming, uses deepseek-reasoner)

use super::types::{ChatResult, FunctionCall, Message, Tool, ToolCall, Usage};
use crate::llm::http_client::LlmHttpClient;
use crate::llm::provider::{LlmClient, Provider};
use crate::llm::truncate_messages_to_budget;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use tracing::{debug, info, instrument, Span};
use uuid::Uuid;

const DEEPSEEK_API_URL: &str = "https://api.deepseek.com/chat/completions";

/// Chat completion request
#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<Tool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<String>, // "auto" | "required" | "none"
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

/// Non-streaming chat response
#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<ResponseChoice>,
    usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
struct ResponseChoice {
    message: ResponseMessage,
}

#[derive(Debug, Deserialize)]
struct ResponseMessage {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    reasoning_content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<ResponseToolCall>>,
}

#[derive(Debug, Deserialize)]
struct ResponseToolCall {
    id: String,
    #[serde(rename = "type")]
    call_type: String,
    function: ResponseFunction,
}

#[derive(Debug, Deserialize)]
struct ResponseFunction {
    name: String,
    arguments: String,
}

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
        let http = LlmHttpClient::new(
            Duration::from_secs(300),
            Duration::from_secs(30),
        );
        Self { api_key, model, http }
    }

    /// Create a new DeepSeek client with a shared HTTP client
    pub fn with_http_client(api_key: String, model: String, client: Client) -> Self {
        Self { api_key, model, http: LlmHttpClient::from_client(client) }
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
    pub async fn chat(&self, messages: Vec<Message>, tools: Option<Vec<Tool>>) -> Result<ChatResult> {
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

        let request = ChatRequest {
            model: self.model.clone(),
            messages,
            tools,
            tool_choice: Some("auto".into()),
            max_tokens: Some(32000),  // Increased for dual-mode: chat for tools, reasoner for synthesis
        };

        let body = serde_json::to_string(&request)?;
        debug!(request_id = %request_id, "DeepSeek request: {}", body);

        let response_body = self.http.execute_with_retry(
            &request_id,
            DEEPSEEK_API_URL,
            &self.api_key,
            body,
        ).await?;

        let data: ChatResponse = serde_json::from_str(&response_body)
            .map_err(|e| anyhow!("Failed to parse DeepSeek response: {}", e))?;

        let duration_ms = start_time.elapsed().as_millis() as u64;

        // Extract response from first choice
        let choice = data.choices.into_iter().next();
        let (content, reasoning_content, tool_calls) = match choice {
            Some(c) => {
                let msg = c.message;
                let tc: Option<Vec<ToolCall>> = msg.tool_calls.map(|calls| {
                    calls
                        .into_iter()
                        .map(|tc| ToolCall {
                            id: tc.id,
                            item_id: None,
                            call_type: tc.call_type,
                            function: FunctionCall {
                                name: tc.function.name,
                                arguments: tc.function.arguments,
                            },
                            thought_signature: None, // DeepSeek doesn't use thought signatures
                        })
                        .collect()
                });
                (msg.content, msg.reasoning_content, tc)
            }
            None => (None, None, None),
        };

        // Log usage stats
        if let Some(ref u) = data.usage {
            // Calculate cache hit ratio if both hit and miss are available
            let cache_hit_ratio = Self::calculate_cache_hit_ratio(u.prompt_cache_hit_tokens, u.prompt_cache_miss_tokens);

            info!(
                request_id = %request_id,
                prompt_tokens = u.prompt_tokens,
                completion_tokens = u.completion_tokens,
                cache_hit = ?u.prompt_cache_hit_tokens,
                cache_miss = ?u.prompt_cache_miss_tokens,
                cache_hit_ratio = ?cache_hit_ratio.map(|r| format!("{:.1}%", r * 100.0)),
                "DeepSeek usage stats"
            );
        }

        // Log tool calls if any
        if let Some(ref tcs) = tool_calls {
            info!(
                request_id = %request_id,
                tool_count = tcs.len(),
                tools = ?tcs.iter().map(|tc| &tc.function.name).collect::<Vec<_>>(),
                "DeepSeek requested tool calls"
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
            content_len = content.as_ref().map(|c| c.len()).unwrap_or(0),
            reasoning_len = reasoning_content.as_ref().map(|r| r.len()).unwrap_or(0),
            tool_calls = tool_calls.as_ref().map(|t| t.len()).unwrap_or(0),
            "DeepSeek chat complete"
        );

        Ok(ChatResult {
            request_id,
            content,
            reasoning_content,
            tool_calls,
            usage: data.usage,
            duration_ms,
        })
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
        assert_eq!(DeepSeekClient::calculate_cache_hit_ratio(Some(100), Some(100)), Some(0.5));
        assert_eq!(DeepSeekClient::calculate_cache_hit_ratio(Some(75), Some(25)), Some(0.75));
        assert_eq!(DeepSeekClient::calculate_cache_hit_ratio(Some(0), Some(100)), Some(0.0));
        assert_eq!(DeepSeekClient::calculate_cache_hit_ratio(Some(100), Some(0)), Some(1.0));
        assert_eq!(DeepSeekClient::calculate_cache_hit_ratio(Some(0), Some(0)), None);
        assert_eq!(DeepSeekClient::calculate_cache_hit_ratio(None, Some(100)), None);
        assert_eq!(DeepSeekClient::calculate_cache_hit_ratio(Some(100), None), None);
        assert_eq!(DeepSeekClient::calculate_cache_hit_ratio(None, None), None);
    }
}
