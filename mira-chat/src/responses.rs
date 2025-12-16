//! GPT-5.2 Responses API client
//!
//! Implements the OpenAI Responses API for GPT-5.2 with:
//! - Variable reasoning effort (none/low/medium/high/xhigh)
//! - Conversation continuity via previous_response_id
//! - Streaming SSE responses
//! - Function calling for tools

use anyhow::Result;
use serde::{Deserialize, Serialize};

const API_URL: &str = "https://api.openai.com/v1/responses";

/// Request to the Responses API
#[derive(Debug, Serialize)]
pub struct ResponsesRequest {
    pub model: String,
    pub input: InputType,
    pub instructions: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_response_id: Option<String>,
    pub reasoning: ReasoningConfig,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<Tool>,
    pub stream: bool,
}

/// Input can be a string or conversation items
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum InputType {
    Text(String),
    Conversation(Vec<ConversationItem>),
}

/// Conversation item for multi-turn with tool results
#[derive(Debug, Clone, Serialize)]
pub struct ConversationItem {
    #[serde(rename = "type")]
    pub item_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
}

/// Reasoning effort configuration
#[derive(Debug, Serialize)]
pub struct ReasoningConfig {
    /// One of: none, low, medium, high, xhigh
    pub effort: String,
}

/// Tool definition for function calling (Responses API format)
#[derive(Debug, Clone, Serialize)]
pub struct Tool {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub parameters: serde_json::Value,
}

/// Response from the Responses API
#[derive(Debug, Deserialize)]
pub struct ResponsesResponse {
    pub id: String,
    pub model: String,
    pub output: Vec<OutputItem>,
    pub usage: Option<Usage>,
    #[serde(default)]
    pub previous_response_id: Option<String>,
}

/// Output item types (polymorphic)
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum OutputItem {
    #[serde(rename = "message")]
    Message {
        id: String,
        status: String,
        content: Vec<ContentItem>,
        role: String,
    },
    #[serde(rename = "function_call")]
    FunctionCall {
        id: String,
        status: String,
        name: String,
        arguments: String,
        call_id: String,
    },
    #[serde(rename = "reasoning")]
    Reasoning {
        id: String,
        summary: Option<String>,
    },
}

/// Content item within a message
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum ContentItem {
    #[serde(rename = "output_text")]
    OutputText { text: String },
    #[serde(rename = "refusal")]
    Refusal { refusal: String },
}

/// Token usage with cache metrics
#[derive(Debug, Deserialize)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    #[serde(default)]
    pub input_tokens_details: Option<InputTokensDetails>,
    #[serde(default)]
    pub output_tokens_details: Option<OutputTokensDetails>,
}

#[derive(Debug, Deserialize, Default)]
pub struct InputTokensDetails {
    #[serde(default)]
    pub cached_tokens: u32,
}

#[derive(Debug, Deserialize, Default)]
pub struct OutputTokensDetails {
    #[serde(default)]
    pub reasoning_tokens: u32,
}

impl Usage {
    pub fn cached_tokens(&self) -> u32 {
        self.input_tokens_details
            .as_ref()
            .map(|d| d.cached_tokens)
            .unwrap_or(0)
    }

    pub fn reasoning_tokens(&self) -> u32 {
        self.output_tokens_details
            .as_ref()
            .map(|d| d.reasoning_tokens)
            .unwrap_or(0)
    }
}

/// GPT-5.2 Responses API client
pub struct Client {
    http: reqwest::Client,
    api_key: String,
}

impl Client {
    /// Create a new client
    pub fn new(api_key: String) -> Self {
        Self {
            http: reqwest::Client::new(),
            api_key,
        }
    }

    /// Create a response (non-streaming)
    pub async fn create(
        &self,
        input: &str,
        instructions: &str,
        previous_response_id: Option<&str>,
        reasoning_effort: &str,
        tools: &[Tool],
    ) -> Result<ResponsesResponse> {
        let request = ResponsesRequest {
            model: "gpt-5.2".into(),
            input: InputType::Text(input.into()),
            instructions: instructions.into(),
            previous_response_id: previous_response_id.map(String::from),
            reasoning: ReasoningConfig {
                effort: reasoning_effort.into(),
            },
            tools: tools.to_vec(),
            stream: false,
        };

        self.send_request(&request).await
    }

    /// Continue a response with tool results
    pub async fn continue_with_tool_results(
        &self,
        previous_response_id: &str,
        tool_results: Vec<(String, String)>, // (call_id, output)
        instructions: &str,
        reasoning_effort: &str,
        tools: &[Tool],
    ) -> Result<ResponsesResponse> {
        let conversation: Vec<ConversationItem> = tool_results
            .into_iter()
            .map(|(call_id, output)| ConversationItem {
                item_type: "function_call_output".into(),
                call_id: Some(call_id),
                output: Some(output),
            })
            .collect();

        let request = ResponsesRequest {
            model: "gpt-5.2".into(),
            input: InputType::Conversation(conversation),
            instructions: instructions.into(),
            previous_response_id: Some(previous_response_id.into()),
            reasoning: ReasoningConfig {
                effort: reasoning_effort.into(),
            },
            tools: tools.to_vec(),
            stream: false,
        };

        self.send_request(&request).await
    }

    async fn send_request(&self, request: &ResponsesRequest) -> Result<ResponsesResponse> {
        let response = self
            .http
            .post(API_URL)
            .bearer_auth(&self.api_key)
            .json(request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await?;
            anyhow::bail!("API error {}: {}", status, body);
        }

        // Get raw text first for debugging
        let text = response.text().await?;

        // Try to parse
        let result: ResponsesResponse = serde_json::from_str(&text)
            .map_err(|e| {
                // Log first 500 chars of response for debugging
                let preview = if text.len() > 500 { &text[..500] } else { &text };
                anyhow::anyhow!("JSON parse error: {}. Response preview: {}", e, preview)
            })?;

        Ok(result)
    }
}

impl OutputItem {
    /// Extract text content from a message
    pub fn text(&self) -> Option<String> {
        match self {
            OutputItem::Message { content, .. } => {
                let texts: Vec<&str> = content
                    .iter()
                    .filter_map(|c| match c {
                        ContentItem::OutputText { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect();
                if texts.is_empty() {
                    None
                } else {
                    Some(texts.join(""))
                }
            }
            _ => None,
        }
    }

    /// Check if this is a function call
    pub fn as_function_call(&self) -> Option<(&str, &str, &str)> {
        match self {
            OutputItem::FunctionCall {
                name,
                arguments,
                call_id,
                ..
            } => Some((name, arguments, call_id)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_serialization() {
        let request = ResponsesRequest {
            model: "gpt-5.2".into(),
            input: InputType::Text("Hello".into()),
            instructions: "Be helpful".into(),
            previous_response_id: None,
            reasoning: ReasoningConfig {
                effort: "medium".into(),
            },
            tools: vec![],
            stream: false,
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("gpt-5.2"));
        assert!(json.contains("medium"));
    }

    #[test]
    fn test_output_item_text() {
        let item = OutputItem::Message {
            id: "msg_123".into(),
            status: "completed".into(),
            content: vec![ContentItem::OutputText {
                text: "Hello world".into(),
            }],
            role: "assistant".into(),
        };

        assert_eq!(item.text(), Some("Hello world".into()));
    }

    #[test]
    fn test_tool_serialization() {
        let tool = Tool {
            tool_type: "function".into(),
            name: "read_file".into(),
            description: Some("Read a file".into()),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"}
                }
            }),
        };

        let json = serde_json::to_string(&tool).unwrap();
        assert!(json.contains("\"type\":\"function\""));
        assert!(json.contains("\"name\":\"read_file\""));
    }
}
