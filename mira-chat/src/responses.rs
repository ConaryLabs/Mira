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
    pub input: String,
    pub instructions: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_response_id: Option<String>,
    pub reasoning: ReasoningConfig,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<Tool>,
    pub stream: bool,
}

/// Reasoning effort configuration
#[derive(Debug, Serialize)]
pub struct ReasoningConfig {
    /// One of: none, low, medium, high, xhigh
    pub effort: String,
}

/// Tool definition for function calling
#[derive(Debug, Clone, Serialize)]
pub struct Tool {
    pub r#type: String,
    pub function: Function,
}

/// Function definition
#[derive(Debug, Clone, Serialize)]
pub struct Function {
    pub name: String,
    pub description: Option<String>,
    pub parameters: serde_json::Value,
}

/// Response from the Responses API
#[derive(Debug, Deserialize)]
pub struct ResponsesResponse {
    pub id: String,
    pub output: Vec<OutputItem>,
    pub usage: Option<Usage>,
}

/// Output item types (polymorphic)
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum OutputItem {
    #[serde(rename = "reasoning")]
    Reasoning { summary: String },
    #[serde(rename = "message")]
    Message { content: String },
    #[serde(rename = "function_call")]
    FunctionCall {
        name: String,
        arguments: String,
        call_id: String,
    },
}

/// Token usage with cache metrics
#[derive(Debug, Deserialize)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cached_input_tokens: u32,
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

    /// Create a response (non-streaming for now)
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
            input: input.into(),
            instructions: instructions.into(),
            previous_response_id: previous_response_id.map(String::from),
            reasoning: ReasoningConfig {
                effort: reasoning_effort.into(),
            },
            tools: tools.to_vec(),
            stream: false, // TODO: implement streaming
        };

        let response = self
            .http
            .post(API_URL)
            .bearer_auth(&self.api_key)
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await?;
            anyhow::bail!("API error {}: {}", status, body);
        }

        let result: ResponsesResponse = response.json().await?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_serialization() {
        let request = ResponsesRequest {
            model: "gpt-5.2".into(),
            input: "Hello".into(),
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
}
