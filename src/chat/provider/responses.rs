//! GPT-5.2 Responses API client
//!
//! Implements the OpenAI Responses API for GPT-5.2 with:
//! - Variable reasoning effort (none/low/medium/high/xhigh)
//! - Conversation continuity via previous_response_id
//! - Streaming SSE responses
//! - Function calling for tools

use anyhow::Result;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

const API_URL: &str = "https://api.openai.com/v1/responses";
const COMPACT_URL: &str = "https://api.openai.com/v1/responses/compact";

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
#[derive(Debug, Clone, Deserialize)]
pub struct ResponsesResponse {
    pub id: String,
    #[serde(default)]
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
        /// Summary can be a string, array, or null
        #[serde(default)]
        summary: serde_json::Value,
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
#[derive(Debug, Clone, Deserialize)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    #[serde(default)]
    pub input_tokens_details: Option<InputTokensDetails>,
    #[serde(default)]
    pub output_tokens_details: Option<OutputTokensDetails>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct InputTokensDetails {
    #[serde(default)]
    pub cached_tokens: u32,
}

#[derive(Debug, Clone, Deserialize, Default)]
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

// ============================================================================
// Streaming types
// ============================================================================

/// Streaming events from the Responses API
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// Text delta - print this immediately
    TextDelta(String),
    /// Function call started
    FunctionCallStart { name: String, call_id: String },
    /// Function call arguments delta
    FunctionCallDelta { call_id: String, arguments_delta: String },
    /// Function call completed
    FunctionCallDone { name: String, call_id: String, arguments: String },
    /// Response completed with final data
    Done(ResponsesResponse),
    /// Error occurred
    Error(String),
}

/// Raw SSE event data
#[derive(Debug, Deserialize)]
struct SseEventData {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    delta: Option<String>,
    #[serde(default)]
    response: Option<ResponsesResponse>,
    #[serde(default)]
    item: Option<StreamOutputItem>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    call_id: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

/// Streaming output item (simplified for parsing)
#[derive(Debug, Clone, Deserialize)]
struct StreamOutputItem {
    #[serde(rename = "type")]
    item_type: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    call_id: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

// ============================================================================
// Client
// ============================================================================

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

    /// Create a streaming response
    pub async fn create_stream(
        &self,
        input: &str,
        instructions: &str,
        previous_response_id: Option<&str>,
        reasoning_effort: &str,
        model: &str,
        tools: &[Tool],
    ) -> Result<mpsc::Receiver<StreamEvent>> {
        let request = ResponsesRequest {
            model: model.into(),
            input: InputType::Text(input.into()),
            instructions: instructions.into(),
            previous_response_id: previous_response_id.map(String::from),
            reasoning: ReasoningConfig {
                effort: reasoning_effort.into(),
            },
            tools: tools.to_vec(),
            stream: true,
        };

        self.send_streaming_request(&request).await
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

    /// Continue with tool results (streaming)
    pub async fn continue_with_tool_results_stream(
        &self,
        previous_response_id: &str,
        tool_results: Vec<(String, String)>,
        instructions: &str,
        reasoning_effort: &str,
        model: &str,
        tools: &[Tool],
    ) -> Result<mpsc::Receiver<StreamEvent>> {
        let conversation: Vec<ConversationItem> = tool_results
            .into_iter()
            .map(|(call_id, output)| ConversationItem {
                item_type: "function_call_output".into(),
                call_id: Some(call_id),
                output: Some(output),
            })
            .collect();

        let request = ResponsesRequest {
            model: model.into(),
            input: InputType::Conversation(conversation),
            instructions: instructions.into(),
            previous_response_id: Some(previous_response_id.into()),
            reasoning: ReasoningConfig {
                effort: reasoning_effort.into(),
            },
            tools: tools.to_vec(),
            stream: true,
        };

        self.send_streaming_request(&request).await
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

        let text = response.text().await?;
        let result: ResponsesResponse = serde_json::from_str(&text)
            .map_err(|e| {
                let preview = if text.len() > 500 { &text[..500] } else { &text };
                anyhow::anyhow!("JSON parse error: {}. Response preview: {}", e, preview)
            })?;

        Ok(result)
    }

    async fn send_streaming_request(
        &self,
        request: &ResponsesRequest,
    ) -> Result<mpsc::Receiver<StreamEvent>> {
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

        let (tx, rx) = mpsc::channel(100);

        // Spawn task to process SSE stream
        let bytes_stream = response.bytes_stream();
        tokio::spawn(async move {
            let mut buffer = String::new();
            let mut current_event = String::new();
            let mut function_calls: std::collections::HashMap<String, (String, String)> =
                std::collections::HashMap::new();

            futures::pin_mut!(bytes_stream);

            while let Some(chunk_result) = bytes_stream.next().await {
                let chunk = match chunk_result {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = tx.send(StreamEvent::Error(e.to_string())).await;
                        break;
                    }
                };

                buffer.push_str(&String::from_utf8_lossy(&chunk));

                // Process complete lines
                while let Some(newline_pos) = buffer.find('\n') {
                    let line = buffer[..newline_pos].trim().to_string();
                    buffer = buffer[newline_pos + 1..].to_string();

                    if line.starts_with("event:") {
                        current_event = line[6..].trim().to_string();
                    } else if line.starts_with("data:") {
                        let data = line[5..].trim();
                        if let Some(event) = parse_sse_event(&current_event, data, &mut function_calls) {
                            if tx.send(event).await.is_err() {
                                return;
                            }
                        }
                    }
                }
            }
        });

        Ok(rx)
    }

    /// Compact conversation context into an encrypted blob
    /// This preserves code-relevant state while reducing token count
    pub async fn compact(
        &self,
        previous_response_id: &str,
        context_description: &str,
    ) -> Result<CompactionResponse> {
        let request = CompactionRequest {
            model: "gpt-5.2".into(),
            previous_response_id: previous_response_id.into(),
            context: context_description.into(),
        };

        let response = self
            .http
            .post(COMPACT_URL)
            .bearer_auth(&self.api_key)
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await?;
            anyhow::bail!("Compaction API error {}: {}", status, body);
        }

        let result: CompactionResponse = response.json().await?;
        Ok(result)
    }

    /// Summarize a list of messages into a concise summary
    /// Uses GPT-5.2 with low reasoning for efficiency
    pub async fn summarize_messages(&self, messages: &[(String, String)]) -> Result<String> {
        // Format messages for summarization
        let formatted: Vec<String> = messages
            .iter()
            .map(|(role, content)| {
                let preview = if content.len() > 500 {
                    // Find a valid char boundary near 500
                    let mut end = 500;
                    while !content.is_char_boundary(end) && end > 0 {
                        end -= 1;
                    }
                    format!("{}...", &content[..end])
                } else {
                    content.clone()
                };
                format!("[{}]: {}", role, preview)
            })
            .collect();

        let prompt = format!(
            "Summarize the following conversation in 2-3 sentences, focusing on key decisions, code changes, and context that would be useful for continuing the conversation later:\n\n{}",
            formatted.join("\n\n")
        );

        let request = ResponsesRequest {
            model: "gpt-5.2".into(),
            input: InputType::Text(prompt),
            instructions: "You are a conversation summarizer. Be concise and focus on actionable context.".into(),
            previous_response_id: None,
            reasoning: ReasoningConfig {
                effort: "low".into(), // Fast summarization
            },
            tools: vec![],
            stream: false,
        };

        let response = self.send_request(&request).await?;

        // Extract text from response
        for item in response.output {
            if let Some(text) = item.text() {
                return Ok(text);
            }
        }

        anyhow::bail!("No text in summarization response")
    }
}

/// Request for compaction
#[derive(Debug, Serialize)]
struct CompactionRequest {
    model: String,
    previous_response_id: String,
    context: String,
}

/// Response from compaction endpoint
#[derive(Debug, Clone, Deserialize)]
pub struct CompactionResponse {
    /// The encrypted compacted content blob
    pub encrypted_content: String,
    /// Approximate tokens saved
    pub tokens_saved: Option<u32>,
}

/// Parse SSE event into StreamEvent
fn parse_sse_event(
    event_type: &str,
    data: &str,
    function_calls: &mut std::collections::HashMap<String, (String, String)>,
) -> Option<StreamEvent> {
    // Try to parse the JSON data
    let parsed: serde_json::Value = serde_json::from_str(data).ok()?;

    match event_type {
        "response.output_text.delta" => {
            // Text streaming
            let delta = parsed.get("delta")?.as_str()?;
            Some(StreamEvent::TextDelta(delta.to_string()))
        }
        "response.function_call_arguments.delta" => {
            // Function call arguments streaming
            let call_id = parsed.get("call_id")?.as_str()?.to_string();
            let delta = parsed.get("delta")?.as_str()?.to_string();

            // Accumulate arguments
            function_calls
                .entry(call_id.clone())
                .or_insert_with(|| (String::new(), String::new()))
                .1
                .push_str(&delta);

            Some(StreamEvent::FunctionCallDelta {
                call_id,
                arguments_delta: delta,
            })
        }
        "response.output_item.added" => {
            // Check if it's a function call
            let item = parsed.get("item")?;
            let item_type = item.get("type")?.as_str()?;

            if item_type == "function_call" {
                let name = item.get("name")?.as_str()?.to_string();
                let call_id = item.get("call_id")?.as_str()?.to_string();

                function_calls.insert(call_id.clone(), (name.clone(), String::new()));

                Some(StreamEvent::FunctionCallStart { name, call_id })
            } else {
                None
            }
        }
        "response.output_item.done" => {
            // Check if it's a completed function call
            let item = parsed.get("item")?;
            let item_type = item.get("type")?.as_str()?;

            if item_type == "function_call" {
                let name = item.get("name")?.as_str()?.to_string();
                let call_id = item.get("call_id")?.as_str()?.to_string();
                let arguments = item.get("arguments")?.as_str()?.to_string();

                Some(StreamEvent::FunctionCallDone { name, call_id, arguments })
            } else {
                None
            }
        }
        "response.completed" => {
            // Final response with usage
            let response = parsed.get("response")?;
            let resp: ResponsesResponse = serde_json::from_value(response.clone()).ok()?;
            Some(StreamEvent::Done(resp))
        }
        _ => None,
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
