// crates/mira-server/src/llm/gemini/client.rs
// Google Gemini 3 Pro API client (non-streaming, supports tool calling)
// Handles internal translation between Mira's format and Google's format
// Note: Built-in tools (Google Search) cannot combine with custom function tools

use crate::http::create_shared_client;
use crate::llm::gemini::conversion::{convert_message, convert_tools, google_search_tool};
use crate::llm::gemini::extraction::{extract_content, extract_thoughts, extract_tool_calls};
use crate::llm::gemini::types::{
    GeminiContent, GeminiRequest, GeminiResponse, GeminiTool, GenerationConfig, ThinkingConfig,
};
use crate::llm::http_client::LlmHttpClient;
use crate::llm::provider::{LlmClient, Provider};
use crate::llm::{ChatResult, Message, Tool, Usage};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use std::time::Instant;
use tracing::{Span, debug, info, instrument};
use uuid::Uuid;

const GEMINI_API_BASE: &str = "https://generativelanguage.googleapis.com/v1beta/models";

/// Default model - use preview for Gemini 3
const DEFAULT_MODEL: &str = "gemini-3-pro-preview";

/// Google Gemini API client
pub struct GeminiClient {
    api_key: String,
    model: String,
    http: LlmHttpClient,
    /// Enable Google Search tool (only when no custom tools provided)
    enable_search: bool,
    /// Thinking level - Pro supports: "low", "high" (default)
    /// Flash also supports: "minimal", "medium"
    thinking_level: String,
}

impl GeminiClient {
    /// Create a new Gemini client with default model
    pub fn new(api_key: String) -> Self {
        Self::with_model(api_key, DEFAULT_MODEL.to_string())
    }

    /// Create a new Gemini client with custom model
    pub fn with_model(api_key: String, model: String) -> Self {
        Self::with_http_client(api_key, model, create_shared_client())
    }

    /// Create a new Gemini client with a shared HTTP client
    pub fn with_http_client(api_key: String, model: String, client: reqwest::Client) -> Self {
        Self {
            api_key,
            model,
            http: LlmHttpClient::from_client(client),
            enable_search: true,
            thinking_level: "high".to_string(),
        }
    }
}

#[async_trait]
impl LlmClient for GeminiClient {
    fn provider_type(&self) -> Provider {
        Provider::Gemini
    }

    fn model_name(&self) -> String {
        self.model.clone()
    }

    /// Gemini budget: 170K tokens (85% of 200K pricing tier)
    fn context_budget(&self) -> u64 {
        170_000
    }

    #[instrument(skip(self, messages, tools), fields(request_id, model = %self.model, message_count = messages.len()))]
    async fn chat(&self, messages: Vec<Message>, tools: Option<Vec<Tool>>) -> Result<ChatResult> {
        let request_id = Uuid::new_v4().to_string();
        let start_time = Instant::now();

        Span::current().record("request_id", &request_id);

        info!(
            request_id = %request_id,
            message_count = messages.len(),
            tool_count = tools.as_ref().map(|t| t.len()).unwrap_or(0),
            model = %self.model,
            thinking_level = %self.thinking_level,
            "Starting Gemini 3 chat request"
        );

        // Build tool call ID to name mapping from assistant messages for correct response formatting
        let mut call_id_to_name: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for msg in &messages {
            if let Some(ref tool_calls) = msg.tool_calls {
                for tc in tool_calls {
                    call_id_to_name.insert(tc.id.clone(), tc.function.name.clone());
                    // Also map item_id if present (for providers with extended tool call tracking)
                    if let Some(ref item_id) = tc.item_id {
                        call_id_to_name.insert(item_id.clone(), tc.function.name.clone());
                    }
                }
            }
        }

        // Convert messages, separating system instruction
        let mut system_instruction: Option<GeminiContent> = None;
        let mut contents: Vec<GeminiContent> = Vec::new();

        for msg in &messages {
            if let Some((content, is_system)) = convert_message(msg, Some(&call_id_to_name)) {
                if is_system {
                    system_instruction = Some(content);
                } else {
                    contents.push(content);
                }
            }
        }

        // Build tools list
        // NOTE: Gemini 3 cannot combine built-in tools with custom function tools
        // Use Google Search only when no custom tools are provided
        let gemini_tools: Option<Vec<GeminiTool>> = if let Some(ref custom_tools) = tools {
            // Custom tools provided - use those (no Google Search)
            Some(vec![convert_tools(custom_tools)])
        } else if self.enable_search {
            // No custom tools - can use Google Search
            Some(vec![google_search_tool()])
        } else {
            None
        };

        let request = GeminiRequest {
            contents,
            system_instruction,
            tools: gemini_tools,
            generation_config: GenerationConfig {
                max_output_tokens: 65536,
                temperature: Some(1.0), // Keep at 1.0 for reasoning per Google docs
                thinking_config: Some(ThinkingConfig {
                    thinking_level: self.thinking_level.clone(),
                    include_thoughts: Some(true), // Get thought summaries for reasoning_content
                }),
            },
        };

        let url = format!(
            "{}/{}:generateContent?key={}",
            GEMINI_API_BASE, self.model, self.api_key
        );

        let body = serde_json::to_string(&request)?;
        debug!(request_id = %request_id, "Gemini request: {}", body);

        // Use shared retry logic — Gemini authenticates via query-string key, not Bearer header
        let response_body = self
            .http
            .execute_request_with_retry(&request_id, body, |client, body| {
                client
                    .post(&url)
                    .header("Content-Type", "application/json")
                    .body(body)
            })
            .await?;

        let duration_ms = start_time.elapsed().as_millis() as u64;

        let data: GeminiResponse = serde_json::from_str(&response_body)
            .map_err(|e| anyhow!("Failed to parse Gemini response: {}", e))?;

        // Extract response from first candidate
        let (content, reasoning_content, tool_calls) = data
            .candidates
            .as_ref()
            .and_then(|c| c.first())
            .map(|candidate| {
                let content = extract_content(&candidate.content);
                let reasoning = extract_thoughts(&candidate.content);
                let tool_calls = extract_tool_calls(&candidate.content);
                (content, reasoning, tool_calls)
            })
            .unwrap_or((None, None, None));

        // Convert usage (Gemini uses different field names)
        let usage = data.usage_metadata.map(|u| Usage {
            prompt_tokens: u.prompt_token_count,
            completion_tokens: u.candidates_token_count.unwrap_or(0),
            total_tokens: u.total_token_count,
            prompt_cache_hit_tokens: None,
            prompt_cache_miss_tokens: None,
        });

        // Log usage stats
        if let Some(ref u) = usage {
            crate::llm::logging::log_usage(&request_id, "Gemini", u);
        }

        if let Some(ref tcs) = tool_calls {
            crate::llm::logging::log_tool_calls(&request_id, "Gemini", tcs);
        }

        crate::llm::logging::log_completion(
            &request_id,
            "Gemini 3",
            duration_ms,
            content.as_ref().map(|c| c.len()).unwrap_or(0),
            reasoning_content.as_ref().map(|r| r.len()).unwrap_or(0),
            tool_calls.as_ref().map(|t| t.len()).unwrap_or(0),
        );

        Ok(ChatResult {
            request_id,
            content,
            reasoning_content, // Gemini 3 thought summaries
            tool_calls,
            usage,
            duration_ms,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================================
    // Constants tests
    // ============================================================================

    #[test]
    fn test_default_model() {
        assert_eq!(DEFAULT_MODEL, "gemini-3-pro-preview");
    }

    #[test]
    fn test_api_base() {
        assert!(GEMINI_API_BASE.contains("googleapis.com"));
    }

    #[test]
    fn test_shared_timeouts() {
        use crate::http::{CONNECT_TIMEOUT, DEFAULT_TIMEOUT};
        use std::time::Duration;
        assert_eq!(DEFAULT_TIMEOUT, Duration::from_secs(300));
        assert_eq!(CONNECT_TIMEOUT, Duration::from_secs(30));
    }

    // ============================================================================
    // GeminiClient creation tests
    // ============================================================================

    #[test]
    fn test_client_new() {
        let client = GeminiClient::new("test-key".to_string());
        assert_eq!(client.model, DEFAULT_MODEL);
        assert_eq!(client.thinking_level, "high");
        assert!(client.enable_search);
    }

    #[test]
    fn test_client_with_model() {
        let client = GeminiClient::with_model("test-key".to_string(), "custom-model".to_string());
        assert_eq!(client.model, "custom-model");
    }
}
