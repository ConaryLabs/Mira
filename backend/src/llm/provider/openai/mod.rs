// src/llm/provider/openai/mod.rs
// OpenAI GPT-5.1 provider implementation

mod conversion;
pub mod embeddings;
pub mod pricing;
pub mod types;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use futures::stream::StreamExt;
use reqwest::Client;
use serde_json::Value;
use std::any::Any;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

use super::{FunctionCall, LlmProvider, Message, Response, TokenUsage, ToolContext, ToolResponse};

// Re-export public types
pub use embeddings::{OpenAIEmbeddingModel, OpenAIEmbeddings};
pub use pricing::{CostResult, OpenAIPricing};
pub use types::OpenAIModel;

// Use helper modules
use conversion::{messages_to_openai, tools_to_openai};
use types::{ChatCompletionRequest, ChatCompletionResponse, ErrorResponse};

/// OpenAI GPT-5.1 provider
#[derive(Clone)]
pub struct OpenAIProvider {
    client: Client,
    api_key: String,
    model: OpenAIModel,
    timeout: Duration,
}

impl OpenAIProvider {
    /// Base URL for OpenAI API
    const BASE_URL: &'static str = "https://api.openai.com/v1";

    /// Create a new OpenAI provider for GPT-5.1
    pub fn gpt51(api_key: String) -> Result<Self> {
        Self::new(api_key, OpenAIModel::Gpt51)
    }

    /// Create a new OpenAI provider for GPT-5.1 Mini
    pub fn gpt51_mini(api_key: String) -> Result<Self> {
        Self::new(api_key, OpenAIModel::Gpt51Mini)
    }

    /// Create a new OpenAI provider with specified model
    pub fn new(api_key: String, model: OpenAIModel) -> Result<Self> {
        if api_key.is_empty() {
            return Err(anyhow!("OpenAI API key is required"));
        }

        let client = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()?;

        Ok(Self {
            client,
            api_key,
            model,
            timeout: Duration::from_secs(120),
        })
    }

    /// Create provider with custom timeout
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Get the model being used
    pub fn model(&self) -> OpenAIModel {
        self.model
    }

    /// Check if provider is configured
    pub fn is_available(&self) -> bool {
        !self.api_key.is_empty()
    }

    /// Validate API key
    pub async fn validate_api_key(&self) -> Result<()> {
        debug!("Validating OpenAI API key");

        let request = ChatCompletionRequest {
            model: self.model.as_str().to_string(),
            messages: vec![types::ChatMessage {
                role: "user".to_string(),
                content: Some("test".to_string()),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            }],
            tools: None,
            tool_choice: None,
            temperature: None,
            max_tokens: Some(1),
            stream: None,
        };

        let response = self
            .client
            .post(format!("{}/chat/completions", Self::BASE_URL))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            let error_msg = match status.as_u16() {
                401 => "Invalid OpenAI API key".to_string(),
                429 => "Rate limit exceeded".to_string(),
                _ => format!("API validation failed ({}): {}", status, error_text),
            };
            return Err(anyhow!(error_msg));
        }

        info!("OpenAI API key validation successful");
        Ok(())
    }

    /// Send a chat completion request
    async fn send_request(&self, request: &ChatCompletionRequest) -> Result<ChatCompletionResponse> {
        debug!(
            "Sending request to OpenAI {} with {} messages",
            self.model,
            request.messages.len()
        );

        let response = self
            .client
            .post(format!("{}/chat/completions", Self::BASE_URL))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .timeout(self.timeout)
            .json(&request)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();

            // Try to parse as OpenAI error response
            if let Ok(error_resp) = serde_json::from_str::<ErrorResponse>(&error_text) {
                return Err(anyhow!(
                    "OpenAI API error ({}): {}",
                    error_resp.error.error_type,
                    error_resp.error.message
                ));
            }

            return Err(anyhow!("OpenAI API returned {}: {}", status, error_text));
        }

        let response_body: ChatCompletionResponse = response.json().await?;
        Ok(response_body)
    }

    /// Parse response into internal format
    fn parse_response(&self, response: ChatCompletionResponse, latency_ms: i64) -> Result<Response> {
        let choice = response
            .choices
            .first()
            .ok_or_else(|| anyhow!("No choices in response"))?;

        let content = choice.message.content.clone().unwrap_or_default();

        let usage = response.usage.as_ref();
        let tokens = TokenUsage {
            input: usage.map(|u| u.prompt_tokens).unwrap_or(0),
            output: usage.map(|u| u.completion_tokens).unwrap_or(0),
            reasoning: 0,
            cached: 0,
        };

        debug!(
            "OpenAI {} response: {} input, {} output tokens",
            self.model, tokens.input, tokens.output
        );

        Ok(Response {
            content,
            model: self.model.as_str().to_string(),
            tokens,
            latency_ms,
        })
    }

    /// Parse tool calling response
    fn parse_tool_response(
        &self,
        response: ChatCompletionResponse,
        latency_ms: i64,
    ) -> Result<ToolResponse> {
        let choice = response
            .choices
            .first()
            .ok_or_else(|| anyhow!("No choices in response"))?;

        let text_output = choice.message.content.clone().unwrap_or_default();

        // Extract function calls
        let function_calls = if let Some(ref tool_calls) = choice.message.tool_calls {
            tool_calls
                .iter()
                .filter_map(|tc| {
                    // Parse arguments from JSON string
                    let arguments: Value = serde_json::from_str(&tc.function.arguments)
                        .unwrap_or_else(|e| {
                            warn!(
                                "Failed to parse tool call arguments: {} - {}",
                                e, tc.function.arguments
                            );
                            Value::Object(serde_json::Map::new())
                        });

                    Some(FunctionCall {
                        id: tc.id.clone(),
                        name: tc.function.name.clone(),
                        arguments,
                    })
                })
                .collect()
        } else {
            Vec::new()
        };

        let usage = response.usage.as_ref();
        let tokens = TokenUsage {
            input: usage.map(|u| u.prompt_tokens).unwrap_or(0),
            output: usage.map(|u| u.completion_tokens).unwrap_or(0),
            reasoning: 0,
            cached: 0,
        };

        debug!(
            "OpenAI {} tool response: {} calls, {} input, {} output tokens",
            self.model,
            function_calls.len(),
            tokens.input,
            tokens.output
        );

        let response_id = response.id.clone();
        let raw = serde_json::to_value(&response).unwrap_or(Value::Null);

        Ok(ToolResponse {
            id: response_id,
            text_output,
            function_calls,
            tokens,
            latency_ms,
            raw_response: raw,
        })
    }

    /// Calculate cost for a response
    pub fn calculate_cost(&self, tokens: &TokenUsage) -> f64 {
        OpenAIPricing::calculate_cost(self.model, tokens.input, tokens.output)
    }
}

#[async_trait]
impl LlmProvider for OpenAIProvider {
    fn name(&self) -> &'static str {
        match self.model {
            OpenAIModel::Gpt51 => "openai-gpt51",
            OpenAIModel::Gpt51Mini => "openai-gpt51-mini",
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    async fn chat(&self, messages: Vec<Message>, system: String) -> Result<Response> {
        let start = Instant::now();

        let openai_messages = messages_to_openai(&messages, &system);

        let request = ChatCompletionRequest {
            model: self.model.as_str().to_string(),
            messages: openai_messages,
            tools: None,
            tool_choice: None,
            temperature: Some(0.7),
            max_tokens: None,
            stream: None,
        };

        let response = self.send_request(&request).await?;
        let latency_ms = start.elapsed().as_millis() as i64;

        self.parse_response(response, latency_ms)
    }

    async fn chat_with_tools(
        &self,
        messages: Vec<Message>,
        system: String,
        tools: Vec<Value>,
        _context: Option<ToolContext>,
    ) -> Result<ToolResponse> {
        let start = Instant::now();

        let openai_messages = messages_to_openai(&messages, &system);
        let openai_tools = tools_to_openai(&tools);

        info!(
            "OpenAI {} chat_with_tools: {} messages, {} tools",
            self.model,
            openai_messages.len(),
            openai_tools.len()
        );

        let request = ChatCompletionRequest {
            model: self.model.as_str().to_string(),
            messages: openai_messages,
            tools: if openai_tools.is_empty() {
                None
            } else {
                Some(openai_tools)
            },
            tool_choice: Some(serde_json::json!("auto")),
            temperature: Some(0.7),
            max_tokens: None,
            stream: None,
        };

        let response = self.send_request(&request).await?;
        let latency_ms = start.elapsed().as_millis() as i64;

        self.parse_tool_response(response, latency_ms)
    }

    async fn stream(
        &self,
        messages: Vec<Message>,
        system: String,
    ) -> Result<Box<dyn futures::Stream<Item = Result<String>> + Send + Unpin>> {
        let openai_messages = messages_to_openai(&messages, &system);

        let request = ChatCompletionRequest {
            model: self.model.as_str().to_string(),
            messages: openai_messages,
            tools: None,
            tool_choice: None,
            temperature: Some(0.7),
            max_tokens: None,
            stream: Some(true),
        };

        let response = self
            .client
            .post(format!("{}/chat/completions", Self::BASE_URL))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .timeout(self.timeout)
            .json(&request)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow!("OpenAI streaming failed ({}): {}", status, error_text));
        }

        // Convert byte stream to text stream
        let byte_stream = response.bytes_stream();

        // Buffer for accumulating partial SSE data
        let stream = byte_stream
            .filter_map(|result| async move {
                match result {
                    Ok(bytes) => {
                        let text = String::from_utf8_lossy(&bytes);
                        // Parse SSE format: "data: {...}\n\n"
                        let mut content = String::new();
                        for line in text.lines() {
                            if let Some(data) = line.strip_prefix("data: ") {
                                if data == "[DONE]" {
                                    continue;
                                }
                                if let Ok(chunk) =
                                    serde_json::from_str::<types::ChatCompletionChunk>(data)
                                {
                                    if let Some(choice) = chunk.choices.first() {
                                        if let Some(ref c) = choice.delta.content {
                                            content.push_str(c);
                                        }
                                    }
                                }
                            }
                        }
                        if content.is_empty() {
                            None
                        } else {
                            Some(Ok(content))
                        }
                    }
                    Err(e) => Some(Err(anyhow!("Stream error: {}", e))),
                }
            })
            .boxed();

        Ok(Box::new(stream))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_creation() {
        let provider = OpenAIProvider::gpt51("test-key".to_string());
        assert!(provider.is_ok());

        let provider = provider.unwrap();
        assert_eq!(provider.model(), OpenAIModel::Gpt51);
        assert!(provider.is_available());
    }

    #[test]
    fn test_provider_creation_mini() {
        let provider = OpenAIProvider::gpt51_mini("test-key".to_string());
        assert!(provider.is_ok());

        let provider = provider.unwrap();
        assert_eq!(provider.model(), OpenAIModel::Gpt51Mini);
        assert_eq!(provider.name(), "openai-gpt51-mini");
    }

    #[test]
    fn test_provider_requires_key() {
        let provider = OpenAIProvider::new("".to_string(), OpenAIModel::Gpt51);
        assert!(provider.is_err());
    }

    #[test]
    fn test_cost_calculation() {
        let provider = OpenAIProvider::gpt51("test-key".to_string()).unwrap();
        let tokens = TokenUsage {
            input: 100_000,
            output: 10_000,
            reasoning: 0,
            cached: 0,
        };

        let cost = provider.calculate_cost(&tokens);
        // GPT-5.1: 0.1 * $1.25 + 0.01 * $10 = $0.225
        assert!((cost - 0.225).abs() < 0.001);
    }
}
