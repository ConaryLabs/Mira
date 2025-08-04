// src/llm/client.rs
use reqwest::{Client, Method, RequestBuilder};
use std::env;
use serde_json::{json, Value};
use anyhow::{Result, Context};

#[derive(Clone)]
pub struct OpenAIClient {
    pub client: Client,
    pub api_key: String,
    pub api_base: String,
}

impl OpenAIClient {
    pub fn new() -> Self {
        let api_key = env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY not set");
        let api_base = env::var("OPENAI_BASE_URL").unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
        
        Self {
            client: Client::new(),
            api_key,
            api_base,
        }
    }
    
    pub fn auth_header(&self) -> (&'static str, String) {
        ("Authorization", format!("Bearer {}", self.api_key))
    }
    
    /// Universal request builder for all OpenAI JSON endpoints
    pub fn request(&self, method: Method, path: &str) -> RequestBuilder {
        self.client
            .request(
                method,
                format!("{}/{}", self.api_base.trim_end_matches('/'), path.trim_start_matches('/')),
            )
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
    }
    
    /// Multipart request builder for file uploads (Content-Type set by reqwest)
    pub fn request_multipart(&self, path: &str) -> RequestBuilder {
        self.client
            .post(format!("{}/{}", self.api_base.trim_end_matches('/'), path.trim_start_matches('/')))
            .header("Authorization", format!("Bearer {}", self.api_key))
        // Don't set Content-Type: multipart is handled by reqwest
    }

    /// Chat completion with function calling support
    pub async fn chat_with_tools(
        &self,
        messages: Vec<Value>,
        tools: Vec<Value>,
        tool_choice: Option<Value>,
        model: Option<&str>,
    ) -> Result<Value> {
        let model = model.unwrap_or("gpt-4.1");
        
        let mut payload = json!({
            "model": model,
            "messages": messages,
            "temperature": 0.7,
        });

        // Add tools if provided
        if !tools.is_empty() {
            payload["tools"] = json!(tools);
            
            // Add tool_choice if specified
            if let Some(choice) = tool_choice {
                payload["tool_choice"] = choice;
            } else {
                // Default to auto
                payload["tool_choice"] = json!("auto");
            }
        }

        let response = self
            .request(Method::POST, "chat/completions")
            .json(&payload)
            .send()
            .await
            .context("Failed to send chat request with tools")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            return Err(anyhow::anyhow!("OpenAI API error {}: {}", status, error_text));
        }

        let response_json: Value = response.json().await.context("Failed to parse response")?;
        Ok(response_json)
    }

    /// Stream chat completion with function calling support
    pub async fn stream_chat_with_tools(
        &self,
        messages: Vec<Value>,
        tools: Vec<Value>,
        tool_choice: Option<Value>,
        model: Option<&str>,
    ) -> Result<impl futures::Stream<Item = Result<Value>>> {
        use futures::StreamExt;
        
        let model = model.unwrap_or("gpt-4.1");
        
        let mut payload = json!({
            "model": model,
            "messages": messages,
            "temperature": 0.7,
            "stream": true,
        });

        // Add tools if provided
        if !tools.is_empty() {
            payload["tools"] = json!(tools);
            
            if let Some(choice) = tool_choice {
                payload["tool_choice"] = choice;
            } else {
                payload["tool_choice"] = json!("auto");
            }
        }

        let response = self
            .request(Method::POST, "chat/completions")
            .json(&payload)
            .send()
            .await
            .context("Failed to send streaming chat request with tools")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            return Err(anyhow::anyhow!("OpenAI API error {}: {}", status, error_text));
        }

        let stream = response.bytes_stream().map(move |chunk| {
            match chunk {
                Ok(bytes) => {
                    let text = String::from_utf8_lossy(&bytes);
                    // Parse SSE format
                    let mut result = json!({});
                    for line in text.lines() {
                        if line.starts_with("data: ") {
                            let data = &line[6..];
                            if data != "[DONE]" {
                                if let Ok(json_data) = serde_json::from_str::<Value>(data) {
                                    result = json_data;
                                    break;
                                }
                            }
                        }
                    }
                    Ok(result)
                }
                Err(e) => Err(anyhow::anyhow!("Stream error: {}", e))
            }
        });

        Ok(stream)
    }

    // Note: simple_chat method is in src/llm/chat.rs, not here
    // This avoids duplicate definitions
}
