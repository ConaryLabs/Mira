// crates/mira-server/src/llm/gemini/client.rs
// Google Gemini 3 Pro API client (non-streaming, supports tool calling)
// Handles internal translation between Mira's format and Google's format
// Note: Built-in tools (Google Search) cannot combine with custom function tools

use crate::llm::deepseek::{ChatResult, FunctionCall, Message, Tool, ToolCall, Usage};
use crate::llm::provider::{LlmClient, Provider};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::{Duration, Instant};
use tracing::{debug, info, instrument, Span};
use uuid::Uuid;

const GEMINI_API_BASE: &str = "https://generativelanguage.googleapis.com/v1beta/models";

/// Request timeout
const REQUEST_TIMEOUT: Duration = Duration::from_secs(300); // 5 minutes
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// Default model - use preview for Gemini 3
const DEFAULT_MODEL: &str = "gemini-3-pro-preview";

// ============================================================================
// Gemini API Types (Google's format)
// ============================================================================

/// Gemini request
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system_instruction: Option<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<GeminiTool>>,
    generation_config: GenerationConfig,
}

/// Thinking configuration for Gemini 3 (nested inside GenerationConfig)
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ThinkingConfig {
    /// Thinking level - Pro supports: "low", "high" (default)
    /// Flash also supports: "minimal", "medium"
    thinking_level: String,
    /// Include thought summaries in response
    #[serde(skip_serializing_if = "Option::is_none")]
    include_thoughts: Option<bool>,
}

/// Gemini content (message)
#[derive(Debug, Clone, Serialize, Deserialize)]
struct GeminiContent {
    role: String, // "user" | "model"
    parts: Vec<GeminiPart>,
}

/// Gemini part (content can have multiple parts)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum GeminiPart {
    Text {
        text: String,
        /// If true, this is a thought summary (reasoning)
        #[serde(default)]
        thought: bool,
    },
    FunctionCall {
        #[serde(rename = "functionCall")]
        function_call: GeminiFunctionCall,
        /// Gemini 3 thought signature - must be preserved and sent back
        #[serde(rename = "thoughtSignature")]
        #[serde(skip_serializing_if = "Option::is_none")]
        #[serde(default)]
        thought_signature: Option<String>,
    },
    FunctionResponse {
        #[serde(rename = "functionResponse")]
        function_response: GeminiFunctionResponse,
    },
}

/// Gemini function call
#[derive(Debug, Clone, Serialize, Deserialize)]
struct GeminiFunctionCall {
    name: String,
    args: Value,
}

/// Gemini function response
#[derive(Debug, Clone, Serialize, Deserialize)]
struct GeminiFunctionResponse {
    name: String,
    response: Value,
}

/// Gemini tool definition - can be functions or built-in tools
#[derive(Debug, Serialize)]
#[serde(untagged)]
enum GeminiTool {
    Functions(GeminiFunctionsTool),
    GoogleSearch(GoogleSearchTool),
}

/// Functions tool wrapper
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiFunctionsTool {
    function_declarations: Vec<GeminiFunctionDeclaration>,
}

/// Google Search built-in tool
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GoogleSearchTool {
    google_search: GoogleSearchConfig,
}

/// Google Search configuration (empty for default)
#[derive(Debug, Serialize)]
struct GoogleSearchConfig {}

/// Gemini function declaration
#[derive(Debug, Serialize)]
struct GeminiFunctionDeclaration {
    name: String,
    description: String,
    parameters: Value,
}

/// Generation config
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GenerationConfig {
    max_output_tokens: u32,
    /// Temperature - keep at 1.0 for reasoning tasks per Google docs
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    /// Thinking configuration (nested)
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking_config: Option<ThinkingConfig>,
}

/// Gemini response
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiResponse {
    candidates: Option<Vec<GeminiCandidate>>,
    usage_metadata: Option<GeminiUsage>,
}

/// Gemini candidate
#[derive(Debug, Deserialize)]
struct GeminiCandidate {
    content: GeminiContent,
}

/// Gemini usage metadata
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiUsage {
    prompt_token_count: u32,
    candidates_token_count: Option<u32>,
    total_token_count: u32,
}

// ============================================================================
// Client Implementation
// ============================================================================

/// Google Gemini API client
pub struct GeminiClient {
    api_key: String,
    model: String,
    client: reqwest::Client,
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
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self::with_http_client(api_key, model, client)
    }

    /// Create a new Gemini client with a shared HTTP client
    pub fn with_http_client(api_key: String, model: String, client: reqwest::Client) -> Self {
        Self {
            api_key,
            model,
            client,
            enable_search: true,
            thinking_level: "high".to_string(),
        }
    }

    /// Convert Mira Message to Gemini Content
    /// Returns (content, is_system) - system messages are handled separately
    fn convert_message(
        msg: &Message,
        tool_id_map: Option<&std::collections::HashMap<String, String>>,
    ) -> Option<(GeminiContent, bool)> {
        match msg.role.as_str() {
            "system" => {
                // System messages go to system_instruction
                let parts = vec![GeminiPart::Text {
                    text: msg.content.clone().unwrap_or_default(),
                    thought: false,
                }];
                Some((
                    GeminiContent {
                        role: "user".into(), // system_instruction uses user role
                        parts,
                    },
                    true,
                ))
            }
            "user" => {
                let parts = vec![GeminiPart::Text {
                    text: msg.content.clone().unwrap_or_default(),
                    thought: false,
                }];
                Some((GeminiContent { role: "user".into(), parts }, false))
            }
            "assistant" => {
                let mut parts = Vec::new();

                // Add text content if present
                if let Some(ref content) = msg.content {
                    if !content.is_empty() {
                        parts.push(GeminiPart::Text {
                            text: content.clone(),
                            thought: false,
                        });
                    }
                }

                // Add function calls if present (include thought signatures for Gemini 3)
                if let Some(ref tool_calls) = msg.tool_calls {
                    for tc in tool_calls {
                        let args: Value =
                            serde_json::from_str(&tc.function.arguments).unwrap_or(Value::Object(Default::default()));
                        parts.push(GeminiPart::FunctionCall {
                            function_call: GeminiFunctionCall {
                                name: tc.function.name.clone(),
                                args,
                            },
                            thought_signature: tc.thought_signature.clone(),
                        });
                    }
                }

                if parts.is_empty() {
                    parts.push(GeminiPart::Text {
                        text: String::new(),
                        thought: false,
                    });
                }

                Some((GeminiContent { role: "model".into(), parts }, false))
            }
            "tool" => {
                // Tool responses become function_response parts
                // We need to find the function name from context
                let tool_call_id = msg.tool_call_id.clone().unwrap_or_default();
                let function_name = tool_id_map
                    .and_then(|m| m.get(&tool_call_id))
                    .cloned()
                    .unwrap_or_else(|| "unknown".into());

                // Gemini requires function_response.response to be a JSON object (Struct),
                // not a string or other primitive. Always ensure we return an object.
                let content_str = msg.content.as_deref().unwrap_or("");
                let response: Value = match serde_json::from_str::<Value>(content_str) {
                    Ok(Value::Object(obj)) => Value::Object(obj),
                    Ok(other) => serde_json::json!({ "result": other }),
                    Err(_) => serde_json::json!({ "result": content_str }),
                };

                let parts = vec![GeminiPart::FunctionResponse {
                    function_response: GeminiFunctionResponse {
                        name: function_name,
                        response,
                    },
                }];

                Some((GeminiContent { role: "user".into(), parts }, false))
            }
            _ => None,
        }
    }

    /// Convert Mira Tool to Gemini FunctionDeclaration
    /// Convert Mira Tools to Gemini function declarations tool
    fn convert_tools(tools: &[Tool]) -> GeminiTool {
        let declarations: Vec<GeminiFunctionDeclaration> = tools
            .iter()
            .map(|t| GeminiFunctionDeclaration {
                name: t.function.name.clone(),
                description: t.function.description.clone(),
                parameters: t.function.parameters.clone(),
            })
            .collect();

        GeminiTool::Functions(GeminiFunctionsTool {
            function_declarations: declarations,
        })
    }

    /// Create Google Search tool
    fn google_search_tool() -> GeminiTool {
        GeminiTool::GoogleSearch(GoogleSearchTool {
            google_search: GoogleSearchConfig {},
        })
    }

    /// Extract tool calls from Gemini response
    fn extract_tool_calls(content: &GeminiContent) -> Option<Vec<ToolCall>> {
        let mut tool_calls = Vec::new();

        for (idx, part) in content.parts.iter().enumerate() {
            if let GeminiPart::FunctionCall { function_call, thought_signature } = part {
                tool_calls.push(ToolCall {
                    id: format!("call_{}", idx),
                    item_id: None,
                    call_type: "function".into(),
                    function: FunctionCall {
                        name: function_call.name.clone(),
                        arguments: serde_json::to_string(&function_call.args).unwrap_or_default(),
                    },
                    thought_signature: thought_signature.clone(),
                });
            }
        }

        if tool_calls.is_empty() {
            None
        } else {
            Some(tool_calls)
        }
    }

    /// Extract text content from Gemini response (non-thought parts only)
    fn extract_content(content: &GeminiContent) -> Option<String> {
        let text_parts: Vec<&str> = content
            .parts
            .iter()
            .filter_map(|part| {
                if let GeminiPart::Text { text, thought } = part {
                    // Only include non-thought text
                    if !thought {
                        Some(text.as_str())
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect();

        if text_parts.is_empty() {
            None
        } else {
            Some(text_parts.join(""))
        }
    }

    /// Extract thought summaries (reasoning) from Gemini response
    fn extract_thoughts(content: &GeminiContent) -> Option<String> {
        let thought_parts: Vec<&str> = content
            .parts
            .iter()
            .filter_map(|part| {
                if let GeminiPart::Text { text, thought } = part {
                    // Only include thought parts
                    if *thought {
                        Some(text.as_str())
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect();

        if thought_parts.is_empty() {
            None
        } else {
            Some(thought_parts.join("\n"))
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
        let mut call_id_to_name: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        for msg in &messages {
            if let Some(ref tool_calls) = msg.tool_calls {
                for tc in tool_calls {
                    call_id_to_name.insert(tc.id.clone(), tc.function.name.clone());
                    // Also map item_id if present (used by OpenAI, but maybe present in shared structs)
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
            if let Some((content, is_system)) = Self::convert_message(msg, Some(&call_id_to_name)) {
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
            Some(vec![Self::convert_tools(custom_tools)])
        } else if self.enable_search {
            // No custom tools - can use Google Search
            Some(vec![Self::google_search_tool()])
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

        let mut attempts = 0;
        let max_attempts = 3;
        let mut backoff = Duration::from_secs(1);

        loop {
            let response_result = self
                .client
                .post(&url)
                .header("Content-Type", "application/json")
                .body(body.clone())
                .send()
                .await;

            match response_result {
                Ok(response) => {
                    let status = response.status();
                    if !status.is_success() {
                        let error_body = response.text().await.unwrap_or_default();
                        
                        // Check for transient errors
                        if attempts < max_attempts && (status.as_u16() == 429 || status.is_server_error()) {
                            tracing::warn!(
                                request_id = %request_id,
                                status = %status,
                                error = %error_body,
                                "Transient error from Gemini, retrying in {:?}...",
                                backoff
                            );
                            tokio::time::sleep(backoff).await;
                            attempts += 1;
                            backoff *= 2;
                            continue;
                        }

                        return Err(anyhow!("Gemini API error {}: {}", status, error_body));
                    }

                    let data: GeminiResponse = response
                        .json()
                        .await
                        .map_err(|e| anyhow!("Failed to parse Gemini response: {}", e))?;

                    let duration_ms = start_time.elapsed().as_millis() as u64;

                    // Extract response from first candidate
                    let (content, reasoning_content, tool_calls) = data
                        .candidates
                        .as_ref()
                        .and_then(|c| c.first())
                        .map(|candidate| {
                            let content = Self::extract_content(&candidate.content);
                            let reasoning = Self::extract_thoughts(&candidate.content);
                            let tool_calls = Self::extract_tool_calls(&candidate.content);
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
                        info!(
                            request_id = %request_id,
                            prompt_tokens = u.prompt_tokens,
                            completion_tokens = u.completion_tokens,
                            total_tokens = u.total_tokens,
                            "Gemini usage stats"
                        );
                    }

                    // Log tool calls if any
                    if let Some(ref tcs) = tool_calls {
                        info!(
                            request_id = %request_id,
                            tool_count = tcs.len(),
                            tools = ?tcs.iter().map(|tc| &tc.function.name).collect::<Vec<_>>(),
                            "Gemini requested tool calls"
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
                        "Gemini 3 chat complete"
                    );

                    return Ok(ChatResult {
                        request_id,
                        content,
                        reasoning_content, // Gemini 3 thought summaries
                        tool_calls,
                        usage,
                        duration_ms,
                    });
                }
                Err(e) => {
                    if attempts < max_attempts {
                        tracing::warn!(
                            request_id = %request_id,
                            error = %e,
                            "Request failed, retrying in {:?}...",
                            backoff
                        );
                        tokio::time::sleep(backoff).await;
                        attempts += 1;
                        backoff *= 2;
                        continue;
                    }
                    return Err(anyhow!("Gemini request failed after retries: {}", e));
                }
            }
        }
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
    fn test_timeouts() {
        assert_eq!(REQUEST_TIMEOUT, Duration::from_secs(300));
        assert_eq!(CONNECT_TIMEOUT, Duration::from_secs(30));
    }

    // ============================================================================
    // GeminiPart serialization tests
    // ============================================================================

    #[test]
    fn test_gemini_part_text_serialize() {
        let part = GeminiPart::Text {
            text: "Hello".to_string(),
            thought: false,
        };
        let json = serde_json::to_string(&part).unwrap();
        assert!(json.contains("\"text\":\"Hello\""));
    }

    #[test]
    fn test_gemini_part_text_thought_serialize() {
        let part = GeminiPart::Text {
            text: "Thinking...".to_string(),
            thought: true,
        };
        let json = serde_json::to_string(&part).unwrap();
        assert!(json.contains("\"thought\":true"));
    }

    #[test]
    fn test_gemini_part_function_call_serialize() {
        let part = GeminiPart::FunctionCall {
            function_call: GeminiFunctionCall {
                name: "search".to_string(),
                args: serde_json::json!({"query": "test"}),
            },
            thought_signature: None,
        };
        let json = serde_json::to_string(&part).unwrap();
        assert!(json.contains("functionCall"));
        assert!(json.contains("search"));
    }

    #[test]
    fn test_gemini_part_function_response_serialize() {
        let part = GeminiPart::FunctionResponse {
            function_response: GeminiFunctionResponse {
                name: "search".to_string(),
                response: serde_json::json!({"result": "found"}),
            },
        };
        let json = serde_json::to_string(&part).unwrap();
        assert!(json.contains("functionResponse"));
    }

    // ============================================================================
    // GeminiContent tests
    // ============================================================================

    #[test]
    fn test_gemini_content_user() {
        let content = GeminiContent {
            role: "user".to_string(),
            parts: vec![GeminiPart::Text {
                text: "Hello".to_string(),
                thought: false,
            }],
        };
        let json = serde_json::to_string(&content).unwrap();
        assert!(json.contains("\"role\":\"user\""));
    }

    #[test]
    fn test_gemini_content_model() {
        let content = GeminiContent {
            role: "model".to_string(),
            parts: vec![GeminiPart::Text {
                text: "Hi there".to_string(),
                thought: false,
            }],
        };
        let json = serde_json::to_string(&content).unwrap();
        assert!(json.contains("\"role\":\"model\""));
    }

    // ============================================================================
    // extract_content tests
    // ============================================================================

    #[test]
    fn test_extract_content_single_text() {
        let content = GeminiContent {
            role: "model".to_string(),
            parts: vec![GeminiPart::Text {
                text: "Hello world".to_string(),
                thought: false,
            }],
        };
        assert_eq!(
            GeminiClient::extract_content(&content),
            Some("Hello world".to_string())
        );
    }

    #[test]
    fn test_extract_content_multiple_texts() {
        let content = GeminiContent {
            role: "model".to_string(),
            parts: vec![
                GeminiPart::Text {
                    text: "Hello ".to_string(),
                    thought: false,
                },
                GeminiPart::Text {
                    text: "world".to_string(),
                    thought: false,
                },
            ],
        };
        assert_eq!(
            GeminiClient::extract_content(&content),
            Some("Hello world".to_string())
        );
    }

    #[test]
    fn test_extract_content_skips_thoughts() {
        let content = GeminiContent {
            role: "model".to_string(),
            parts: vec![
                GeminiPart::Text {
                    text: "I'm thinking...".to_string(),
                    thought: true,
                },
                GeminiPart::Text {
                    text: "Here's the answer".to_string(),
                    thought: false,
                },
            ],
        };
        assert_eq!(
            GeminiClient::extract_content(&content),
            Some("Here's the answer".to_string())
        );
    }

    #[test]
    fn test_extract_content_only_thoughts_returns_none() {
        let content = GeminiContent {
            role: "model".to_string(),
            parts: vec![GeminiPart::Text {
                text: "Thinking...".to_string(),
                thought: true,
            }],
        };
        assert_eq!(GeminiClient::extract_content(&content), None);
    }

    #[test]
    fn test_extract_content_empty_parts() {
        let content = GeminiContent {
            role: "model".to_string(),
            parts: vec![],
        };
        assert_eq!(GeminiClient::extract_content(&content), None);
    }

    #[test]
    fn test_extract_content_skips_function_calls() {
        let content = GeminiContent {
            role: "model".to_string(),
            parts: vec![
                GeminiPart::Text {
                    text: "Let me search".to_string(),
                    thought: false,
                },
                GeminiPart::FunctionCall {
                    function_call: GeminiFunctionCall {
                        name: "search".to_string(),
                        args: serde_json::json!({}),
                    },
                    thought_signature: None,
                },
            ],
        };
        assert_eq!(
            GeminiClient::extract_content(&content),
            Some("Let me search".to_string())
        );
    }

    // ============================================================================
    // extract_thoughts tests
    // ============================================================================

    #[test]
    fn test_extract_thoughts_single() {
        let content = GeminiContent {
            role: "model".to_string(),
            parts: vec![GeminiPart::Text {
                text: "Analyzing the problem...".to_string(),
                thought: true,
            }],
        };
        assert_eq!(
            GeminiClient::extract_thoughts(&content),
            Some("Analyzing the problem...".to_string())
        );
    }

    #[test]
    fn test_extract_thoughts_multiple() {
        let content = GeminiContent {
            role: "model".to_string(),
            parts: vec![
                GeminiPart::Text {
                    text: "First thought".to_string(),
                    thought: true,
                },
                GeminiPart::Text {
                    text: "Second thought".to_string(),
                    thought: true,
                },
            ],
        };
        let thoughts = GeminiClient::extract_thoughts(&content).unwrap();
        assert!(thoughts.contains("First thought"));
        assert!(thoughts.contains("Second thought"));
    }

    #[test]
    fn test_extract_thoughts_skips_non_thoughts() {
        let content = GeminiContent {
            role: "model".to_string(),
            parts: vec![
                GeminiPart::Text {
                    text: "Thinking...".to_string(),
                    thought: true,
                },
                GeminiPart::Text {
                    text: "Final answer".to_string(),
                    thought: false,
                },
            ],
        };
        assert_eq!(
            GeminiClient::extract_thoughts(&content),
            Some("Thinking...".to_string())
        );
    }

    #[test]
    fn test_extract_thoughts_no_thoughts_returns_none() {
        let content = GeminiContent {
            role: "model".to_string(),
            parts: vec![GeminiPart::Text {
                text: "Just an answer".to_string(),
                thought: false,
            }],
        };
        assert_eq!(GeminiClient::extract_thoughts(&content), None);
    }

    // ============================================================================
    // extract_tool_calls tests
    // ============================================================================

    #[test]
    fn test_extract_tool_calls_single() {
        let content = GeminiContent {
            role: "model".to_string(),
            parts: vec![GeminiPart::FunctionCall {
                function_call: GeminiFunctionCall {
                    name: "search".to_string(),
                    args: serde_json::json!({"query": "test"}),
                },
                thought_signature: None,
            }],
        };
        let calls = GeminiClient::extract_tool_calls(&content).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.name, "search");
    }

    #[test]
    fn test_extract_tool_calls_multiple() {
        let content = GeminiContent {
            role: "model".to_string(),
            parts: vec![
                GeminiPart::FunctionCall {
                    function_call: GeminiFunctionCall {
                        name: "search".to_string(),
                        args: serde_json::json!({"q": "1"}),
                    },
                    thought_signature: None,
                },
                GeminiPart::FunctionCall {
                    function_call: GeminiFunctionCall {
                        name: "read".to_string(),
                        args: serde_json::json!({"path": "/tmp"}),
                    },
                    thought_signature: Some("sig123".to_string()),
                },
            ],
        };
        let calls = GeminiClient::extract_tool_calls(&content).unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].function.name, "search");
        assert_eq!(calls[1].function.name, "read");
        assert_eq!(calls[1].thought_signature, Some("sig123".to_string()));
    }

    #[test]
    fn test_extract_tool_calls_none_when_no_calls() {
        let content = GeminiContent {
            role: "model".to_string(),
            parts: vec![GeminiPart::Text {
                text: "Just text".to_string(),
                thought: false,
            }],
        };
        assert_eq!(GeminiClient::extract_tool_calls(&content), None);
    }

    #[test]
    fn test_extract_tool_calls_generates_ids() {
        let content = GeminiContent {
            role: "model".to_string(),
            parts: vec![
                GeminiPart::FunctionCall {
                    function_call: GeminiFunctionCall {
                        name: "func1".to_string(),
                        args: serde_json::json!({}),
                    },
                    thought_signature: None,
                },
                GeminiPart::FunctionCall {
                    function_call: GeminiFunctionCall {
                        name: "func2".to_string(),
                        args: serde_json::json!({}),
                    },
                    thought_signature: None,
                },
            ],
        };
        let calls = GeminiClient::extract_tool_calls(&content).unwrap();
        assert_eq!(calls[0].id, "call_0");
        assert_eq!(calls[1].id, "call_1");
    }

    // ============================================================================
    // convert_tools tests
    // ============================================================================

    #[test]
    fn test_convert_tools_single() {
        let tools = vec![Tool {
            tool_type: "function".to_string(),
            function: crate::llm::deepseek::FunctionDef {
                name: "search".to_string(),
                description: "Search for things".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {}
                }),
            },
        }];
        let result = GeminiClient::convert_tools(&tools);
        match result {
            GeminiTool::Functions(funcs) => {
                assert_eq!(funcs.function_declarations.len(), 1);
                assert_eq!(funcs.function_declarations[0].name, "search");
            }
            _ => panic!("Expected Functions tool"),
        }
    }

    #[test]
    fn test_convert_tools_multiple() {
        let tools = vec![
            Tool {
                tool_type: "function".to_string(),
                function: crate::llm::deepseek::FunctionDef {
                    name: "search".to_string(),
                    description: "Search".to_string(),
                    parameters: serde_json::json!({}),
                },
            },
            Tool {
                tool_type: "function".to_string(),
                function: crate::llm::deepseek::FunctionDef {
                    name: "read".to_string(),
                    description: "Read".to_string(),
                    parameters: serde_json::json!({}),
                },
            },
        ];
        let result = GeminiClient::convert_tools(&tools);
        match result {
            GeminiTool::Functions(funcs) => {
                assert_eq!(funcs.function_declarations.len(), 2);
            }
            _ => panic!("Expected Functions tool"),
        }
    }

    // ============================================================================
    // google_search_tool tests
    // ============================================================================

    #[test]
    fn test_google_search_tool_creation() {
        let tool = GeminiClient::google_search_tool();
        match tool {
            GeminiTool::GoogleSearch(_) => {} // Expected
            _ => panic!("Expected GoogleSearch tool"),
        }
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

    // ============================================================================
    // ThinkingConfig tests
    // ============================================================================

    #[test]
    fn test_thinking_config_serialize() {
        let config = ThinkingConfig {
            thinking_level: "high".to_string(),
            include_thoughts: Some(true),
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("thinkingLevel"));
        assert!(json.contains("includeThoughts"));
    }

    // ============================================================================
    // GenerationConfig tests
    // ============================================================================

    #[test]
    fn test_generation_config_serialize() {
        let config = GenerationConfig {
            max_output_tokens: 8192,
            temperature: Some(1.0),
            thinking_config: None,
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("maxOutputTokens"));
        assert!(json.contains("8192"));
    }
}
