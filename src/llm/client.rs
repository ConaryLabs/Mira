// src/llm/client.rs
// Phase 5-9: GPT-5 Responses API client implementation

use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{info, debug};

/// Extract text from GPT-5 responses JSON
pub fn extract_text_from_responses(resp_json: &serde_json::Value) -> Option<String> {
    // Try unified output format first
    if let Some(output) = resp_json.get("output").and_then(|o| o.as_array()) {
        for item in output {
            if item.get("type").and_then(|t| t.as_str()) == Some("message") {
                if let Some(content) = item.get("content").and_then(|c| c.as_array()) {
                    for block in content {
                        if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                            if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                return Some(text.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    // Try choices format (older format)
    if let Some(choices) = resp_json.get("choices").and_then(|c| c.as_array()) {
        if let Some(first_choice) = choices.first() {
            if let Some(message) = first_choice.get("message") {
                if let Some(content) = message.get("content").and_then(|c| c.as_str()) {
                    return Some(content.to_string());
                }
            }
        }
    }

    None
}

/// Main OpenAI client for GPT-5 Responses API
pub struct OpenAIClient {
    pub client: Client,
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    pub verbosity: String,
    pub reasoning_effort: String,
    pub max_output_tokens: usize,
}

impl OpenAIClient {
    /// Create a new OpenAI client configured for GPT-5
    pub fn new() -> Result<Arc<Self>> {
        let api_key = std::env::var("OPENAI_API_KEY")?;
        let model = std::env::var("MIRA_MODEL")
            .unwrap_or_else(|_| "gpt-5".to_string());
        let verbosity = std::env::var("MIRA_VERBOSITY")
            .unwrap_or_else(|_| "medium".to_string());
        let reasoning_effort = std::env::var("MIRA_REASONING_EFFORT")
            .unwrap_or_else(|_| "medium".to_string());
        let max_output_tokens = std::env::var("MIRA_MAX_OUTPUT_TOKENS")
            .unwrap_or_else(|_| "1024".to_string())
            .parse()
            .unwrap_or(1024);

        info!(
            "🚀 Initializing GPT-5 client (model={}, verbosity={}, reasoning={})",
            model, verbosity, reasoning_effort
        );

        Ok(Arc::new(Self {
            client: Client::new(),
            api_key,
            base_url: "https://api.openai.com".to_string(),
            model,
            verbosity,
            reasoning_effort,
            max_output_tokens,
        }))
    }

    /// Generate a response using the GPT-5 Responses API
    pub async fn generate_response(
        &self,
        user_text: &str,
        system_prompt: Option<&str>,
        request_structured: bool,
    ) -> Result<ResponseOutput> {
        let mut input = vec![
            InputMessage {
                role: "user".to_string(),
                content: vec![
                    ContentBlock::Text {
                        text_type: "input_text".to_string(),
                        text: user_text.to_string(),
                    }
                ],
            }
        ];

        // Add system prompt if provided
        if let Some(system) = system_prompt {
            input.insert(0, InputMessage {
                role: "system".to_string(),
                content: vec![
                    ContentBlock::Text {
                        text_type: "input_text".to_string(),
                        text: system.to_string(),
                    }
                ],
            });
        }

        // NOTE: `response_format` is deprecated for the Responses API.
        // Use `text.format` instead (e.g., "json_object") when you want structured output.
        let request = ResponseRequest {
            model: self.model.clone(),
            input,
            parameters: Parameters {
                verbosity: self.verbosity.clone(),
                reasoning_effort: self.reasoning_effort.clone(),
                max_output_tokens: self.max_output_tokens,
            },
            text: if request_structured {
                Some(TextOptions { format: Some("json_object".to_string()) })
            } else {
                None
            },
        };

        debug!("📤 Sending request to GPT-5 Responses API");

        let response = self.client
            .post(format!("{}/v1/responses", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|_| "<no body>".into());
            return Err(anyhow::anyhow!("OpenAI API error ({}): {}", status, error_text));
        }

        let api_response: ResponseApiResponse = response.json().await?;

        // Extract the text from the unified output format
        let output_text = api_response.output
            .iter()
            .filter_map(|item| {
                if item.output_type == "message" {
                    item.content.as_ref().and_then(|content| {
                        content.iter()
                            .filter_map(|block| {
                                let ContentBlock::Text { text, .. } = block;
                                Some(text.clone())
                            })
                            .next()
                    })
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        Ok(ResponseOutput {
            output: output_text,
            reasoning_summary: api_response.reasoning_summary,
        })
    }

    /// Get embeddings for text using text-embedding-3-large
    pub async fn get_embedding(&self, text: &str) -> Result<Vec<f32>> {
        let request = EmbeddingRequest {
            model: "text-embedding-3-large".to_string(),
            input: text.to_string(),
            dimensions: Some(3072),
        };

        let response = self.client
            .post(format!("{}/v1/embeddings", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|_| "<no body>".into());
            return Err(anyhow::anyhow!("Embedding API error ({}): {}", status, error_text));
        }

        let api_response: EmbeddingResponse = response.json().await?;
        
        Ok(api_response.data
            .into_iter()
            .next()
            .map(|d| d.embedding)
            .unwrap_or_default())
    }

    /// Helper method for making POST requests (used by other modules)
    pub async fn post_response(&self, body: serde_json::Value) -> Result<serde_json::Value> {
        let response = self.client
            .post(format!("{}/v1/responses", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|_| "<no body>".into());
            return Err(anyhow::anyhow!("OpenAI API error ({}): {}", status, error_text));
        }

        Ok(response.json().await?)
    }

    /// Helper method for making generic requests (used by other modules)
    pub fn request(&self, method: reqwest::Method, endpoint: &str) -> reqwest::RequestBuilder {
        self.client
            .request(method, format!("{}/v1/{}", self.base_url, endpoint))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
    }

    /// Helper method for multipart requests (used for file uploads)
    pub fn request_multipart(&self, endpoint: &str) -> reqwest::RequestBuilder {
        self.client
            .post(format!("{}/v1/{}", self.base_url, endpoint))
            .header("Authorization", format!("Bearer {}", self.api_key))
    }
}

// Request/Response types for GPT-5 Responses API

#[derive(Serialize)]
struct ResponseRequest {
    model: String,
    input: Vec<InputMessage>,
    parameters: Parameters,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<TextOptions>, // <-- replaces deprecated `response_format`
}

#[derive(Serialize)]
struct InputMessage {
    role: String,
    content: Vec<ContentBlock>,
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum ContentBlock {
    Text {
        #[serde(rename = "type")]
        text_type: String,
        text: String,
    },
}

#[derive(Serialize)]
struct Parameters {
    verbosity: String,
    reasoning_effort: String,
    max_output_tokens: usize,
}

#[derive(Serialize)]
struct TextOptions {
    // e.g. "json_object" when you want a JSON object back
    #[serde(skip_serializing_if = "Option::is_none")]
    format: Option<String>,
}

#[derive(Deserialize)]
struct ResponseApiResponse {
    output: Vec<OutputItem>,
    #[serde(default)]
    reasoning_summary: Option<String>,
}

#[derive(Deserialize)]
struct OutputItem {
    #[serde(rename = "type")]
    output_type: String,
    #[serde(default)]
    content: Option<Vec<ContentBlock>>,
}

/// Response output from GPT-5
pub struct ResponseOutput {
    pub output: String,
    pub reasoning_summary: Option<String>,
}

// Embedding API types

#[derive(Serialize)]
struct EmbeddingRequest {
    model: String,
    input: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    dimensions: Option<usize>,
}

#[derive(Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}
