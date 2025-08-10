// src/llm/anthropic_client.rs

use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use futures::stream::{Stream, StreamExt};
use std::{env, pin::Pin, time::Duration};
use tokio::time::sleep;

pub struct AnthropicClient {
    client: Client,
    api_key: String,
    beta_features: Vec<String>,
}

impl AnthropicClient {
    pub fn new() -> Self {
        let api_key = env::var("ANTHROPIC_API_KEY")
            .expect("ANTHROPIC_API_KEY must be set");
        
        // Enable ALL beta features as of August 2025
        let beta_features = vec![
            "prompt-caching-2024-07-31".to_string(),
            "computer-use-2024-10-22".to_string(),
            "pdfs-2024-09-25".to_string(),
            "token-counting-2024-11-01".to_string(),
            "max-tokens-3-5-sonnet-2024-07-15".to_string(),
            "message-batches-2024-09-24".to_string(),
            "streaming-2025-08-01".to_string(),  // Latest streaming with tool use
            "vision-2025-08-01".to_string(),     // Enhanced vision capabilities
            "parallel-tools-2025-07-15".to_string(), // Parallel tool execution
        ];
        
        eprintln!("🧠 Anthropic initialized with features: {:?}", beta_features);
        
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(300))
                .build()
                .unwrap(),
            api_key,
            beta_features,
        }
    }

    pub async fn create_message(&self, request: MessageRequest) -> Result<MessageResponse> {
        let mut attempt = 0;
        let max_attempts = 3;
        
        loop {
            let response = self.client
                .post("https://api.anthropic.com/v1/messages")
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", "2023-06-01")
                .header("anthropic-beta", self.beta_features.join(","))
                .json(&request)
                .send()
                .await?;
            
            match response.status().as_u16() {
                200 => return Ok(response.json::<MessageResponse>().await?),
                429 => {
                    // Rate limit - exponential backoff
                    attempt += 1;
                    if attempt >= max_attempts {
                        return Err(anyhow::anyhow!("Rate limited after {} attempts", max_attempts));
                    }
                    let wait_time = Duration::from_secs(2u64.pow(attempt));
                    eprintln!("⏳ Rate limited, waiting {:?}...", wait_time);
                    sleep(wait_time).await;
                },
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
        request.stream = Some(true);
        
        let response = self.client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("anthropic-beta", self.beta_features.join(","))
            .json(&request)
            .send()
            .await?;
        
        if !response.status().is_success() {
            let error_body = response.text().await?;
            return Err(anyhow::anyhow!("Stream API error: {}", error_body));
        }
        
        let stream = response.bytes_stream().map(|item| {
            match item {
                Ok(bytes) => {
                    let text = String::from_utf8_lossy(&bytes);
                    // Parse SSE format
                    if text.starts_with("data: ") {
                        let json_str = text.trim_start_matches("data: ").trim();
                        serde_json::from_str::<StreamEvent>(json_str)
                            .map_err(|e| anyhow::anyhow!("Parse error: {}", e))
                    } else {
                        Ok(StreamEvent::Ping)
                    }
                },
                Err(e) => Err(anyhow::anyhow!("Stream error: {}", e))
            }
        });
        
        Ok(Box::pin(stream))
    }

    pub async fn count_tokens(&self, messages: Vec<Message>) -> Result<TokenCount> {
        let response = self.client
            .post("https://api.anthropic.com/v1/messages/count_tokens")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("anthropic-beta", "token-counting-2024-11-01")
            .json(&json!({
                "model": "claude-sonnet-4-0",
                "messages": messages
            }))
            .send()
            .await?;
        
        Ok(response.json::<TokenCount>().await?)
    }

    pub async fn create_batch(&self, requests: Vec<MessageRequest>) -> Result<BatchResponse> {
        let batch_requests: Vec<_> = requests.into_iter().enumerate().map(|(idx, req)| {
            json!({
                "custom_id": format!("req_{}", idx),
                "params": req
            })
        }).collect();
        
        let response = self.client
            .post("https://api.anthropic.com/v1/messages/batches")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("anthropic-beta", "message-batches-2024-09-24")
            .json(&json!({
                "requests": batch_requests
            }))
            .send()
            .await?;
        
        Ok(response.json::<BatchResponse>().await?)
    }
}

// Request/Response types
#[derive(Debug, Serialize, Clone)]
pub struct MessageRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub max_tokens: u32,
    pub temperature: Option<f32>,
    pub system: Option<String>,
    pub stream: Option<bool>,
    pub tools: Option<Vec<Tool>>,
    pub tool_choice: Option<ToolChoice>,
    pub metadata: Option<Value>,
}

impl Default for MessageRequest {
    fn default() -> Self {
        Self {
            model: "claude-sonnet-4-0".to_string(),
            messages: vec![],
            max_tokens: 100000,  // No limits - you'll handle in billing
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
        self.content.iter()
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
