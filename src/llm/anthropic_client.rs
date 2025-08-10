use anyhow::Result;
use futures::stream::{Stream, StreamExt};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{env, pin::Pin, time::Duration};
use tokio::time::sleep;

pub struct AnthropicClient {
    client: Client,
    api_key: String,
}

impl AnthropicClient {
    pub fn new() -> Self {
        let api_key = env::var("ANTHROPIC_API_KEY")
            .expect("ANTHROPIC_API_KEY must be set");

        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(300))
                .build()
                .unwrap(),
            api_key,
        }
    }

    /// Clamp max_tokens to the model's known output cap.
    /// Defaults conservatively if unknown.
    fn clamp_max_tokens(model: &str, requested: u32) -> u32 {
        let m = model.to_ascii_lowercase();

        // Known caps (output tokens)
        let cap = if m.contains("sonnet-4") {
            64_000
        } else if m.contains("opus-4-1") || m.contains("opus4-1") {
            32_000
        } else if m.contains("opus-4") {
            // Opus 4 (non-4.1) commonly 32k output on most platforms
            32_000
        } else {
            // Unknown/new model: be safe
            8_192
        };

        requested.min(cap).max(1)
    }

    /// Normalize request so it's always API-legal:
    /// - If tools are None or empty, strip tool_choice as well.
    /// - Clamp max_tokens to per-model caps to avoid 400s.
    fn normalize_request(mut req: MessageRequest) -> MessageRequest {
        let tools_empty = req.tools.as_ref().map(|v| v.is_empty()).unwrap_or(true);
        if tools_empty {
            req.tools = None;
            req.tool_choice = None;
        }
        req.max_tokens = Self::clamp_max_tokens(&req.model, req.max_tokens);
        req
    }

    pub async fn create_message(&self, request: MessageRequest) -> Result<MessageResponse> {
        let mut attempt = 0;
        let max_attempts = 3;

        // Ensure request is valid before sending
        let request = Self::normalize_request(request);

        loop {
            let response = self
                .client
                .post("https://api.anthropic.com/v1/messages")
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", "2023-06-01")
                .json(&request)
                .send()
                .await?;

            match response.status().as_u16() {
                200 => return Ok(response.json::<MessageResponse>().await?),
                429 => {
                    attempt += 1;
                    if attempt >= max_attempts {
                        return Err(anyhow::anyhow!(
                            "Rate limited after {} attempts",
                            max_attempts
                        ));
                    }
                    let wait_time = Duration::from_secs(2u64.pow(attempt));
                    eprintln!("⏳ Rate limited, waiting {:?}...", wait_time);
                    sleep(wait_time).await;
                }
                code => {
                    let error_body = response.text().await?;
                    return Err(anyhow::anyhow!("API error {}: {}", code, error_body));
                }
            }
        }
    }

    pub async fn create_message_stream(
        &self,
        mut request: MessageRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
        // Normalize and enable streaming
        request = Self::normalize_request(request);
        request.stream = Some(true);

        let response = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_body = response.text().await?;
            return Err(anyhow::anyhow!("Stream API error: {}", error_body));
        }

        // Robust SSE parsing (event+data, multiline, buffering)
        let mut inner = response.bytes_stream();

        let s = futures::stream::unfold(
            (String::new(), inner),
            |mut state: (String, _)| async move {
                loop {
                    if let Some(pos) = state.0.find("\n\n") {
                        let frame = state.0[..pos].to_string();
                        state.0 = state.0[pos + 2..].to_string();

                        let mut _event_name = String::new();
                        let mut data_lines: Vec<String> = Vec::new();

                        for line in frame.lines() {
                            if line.starts_with(':') {
                                continue; // comment/heartbeat
                            }
                            if let Some(rest) = line.strip_prefix("event:") {
                                _event_name = rest.trim().to_string();
                                continue;
                            }
                            if let Some(rest) = line.strip_prefix("data:") {
                                data_lines.push(rest.trim().to_string());
                                continue;
                            }
                            if line.trim_start().starts_with('{') {
                                data_lines.push(line.trim().to_string());
                            }
                        }

                        if !data_lines.is_empty() {
                            let data = data_lines.join("\n");
                            let parsed = serde_json::from_str::<StreamEvent>(&data)
                                .map_err(|e| anyhow::anyhow!("Parse error: {}", e));
                            return Some((parsed, state));
                        } else {
                            return Some((Ok(StreamEvent::Ping), state));
                        }
                    }

                    match state.1.next().await {
                        Some(Ok(bytes)) => {
                            state.0.push_str(&String::from_utf8_lossy(&bytes));
                            continue;
                        }
                        Some(Err(e)) => {
                            return Some((Err(anyhow::anyhow!("Stream error: {}", e)), state))
                        }
                        None => {
                            if !state.0.trim().is_empty() {
                                let frame = std::mem::take(&mut state.0);
                                let mut _event_name = String::new();
                                let mut data_lines: Vec<String> = Vec::new();
                                for line in frame.lines() {
                                    if line.starts_with(':') {
                                        continue;
                                    }
                                    if let Some(rest) = line.strip_prefix("event:") {
                                        _event_name = rest.trim().to_string();
                                        continue;
                                    }
                                    if let Some(rest) = line.strip_prefix("data:") {
                                        data_lines.push(rest.trim().to_string());
                                        continue;
                                    }
                                    if line.trim_start().starts_with('{') {
                                        data_lines.push(line.trim().to_string());
                                    }
                                }
                                if !data_lines.is_empty() {
                                    let data = data_lines.join("\n");
                                    let parsed = serde_json::from_str::<StreamEvent>(&data)
                                        .map_err(|e| anyhow::anyhow!("Parse error: {}", e));
                                    return Some((parsed, state));
                                }
                            }
                            return None;
                        }
                    }
                }
            },
        );

        Ok(Box::pin(s))
    }

    pub async fn count_tokens(&self, messages: Vec<Message>) -> Result<TokenCount> {
        let response = self
            .client
            .post("https://api.anthropic.com/v1/messages/count_tokens")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&json!({
                "model": "claude-sonnet-4-0", // alias is fine
                "messages": messages
            }))
            .send()
            .await?;

        Ok(response.json::<TokenCount>().await?)
    }

    pub async fn create_batch(&self, requests: Vec<MessageRequest>) -> Result<BatchResponse> {
        let batch_requests: Vec<_> = requests
            .into_iter()
            .map(Self::normalize_request)
            .enumerate()
            .map(|(idx, req)| {
                json!({
                    "custom_id": format!("req_{}", idx),
                    "params": req
                })
            })
            .collect();

        let response = self
            .client
            .post("https://api.anthropic.com/v1/messages/batches")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&json!({
                "requests": batch_requests
            }))
            .send()
            .await?;

        Ok(response.json::<BatchResponse>().await?)
    }
}

// ----- Types -----

#[derive(Debug, Serialize, Clone)]
pub struct MessageRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

impl Default for MessageRequest {
    fn default() -> Self {
        Self {
            model: "claude-sonnet-4-0".to_string(), // keep alias for now
            messages: vec![],
            max_tokens: 4096, // safer default; will be clamped up to caps if raised
            temperature: Some(0.7),
            system: None,
            stream: None,
            tools: None,
            tool_choice: None,
            metadata: None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Message {
    pub role: String,
    pub content: MessageContent,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
    },
    #[serde(rename = "image")]
    Image {
        source: ImageSource,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
    },
    #[serde(rename = "document")]
    Document {
        source: DocumentSource,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
    },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ImageSource {
    #[serde(rename = "type")]
    pub source_type: String,
    pub media_type: String,
    pub data: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DocumentSource {
    #[serde(rename = "type")]
    pub source_type: String,
    pub media_type: String,
    pub data: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CacheControl {
    #[serde(rename = "type")]
    pub cache_type: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MessageResponse {
    pub id: String,
    pub content: Vec<ContentBlock>,
    pub model: String,
    pub role: String,
    pub stop_reason: Option<String>,
    pub usage: Usage,
}

impl MessageResponse {
    pub fn get_text(&self) -> String {
        self.content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_creation_input_tokens: Option<u32>,
    pub cache_read_input_tokens: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Tool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum ToolChoice {
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "any")]
    Any,
    #[serde(rename = "tool")]
    Tool { name: String },
}

#[derive(Debug, Deserialize)]
pub struct StreamEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub index: Option<usize>,
    pub delta: Option<Delta>,
}

impl StreamEvent {
    pub const Ping: StreamEvent = StreamEvent {
        event_type: String::new(),
        index: None,
        delta: None,
    };
}

#[derive(Debug, Deserialize)]
pub struct Delta {
    pub text: Option<String>,
    pub stop_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TokenCount {
    pub input_tokens: u32,
}

#[derive(Debug, Deserialize)]
pub struct BatchResponse {
    pub id: String,
    pub processing_status: String,
    pub created_at: String,
}
