// src/llm/client/mod.rs

use std::sync::Arc;

use anyhow::Result;
use reqwest::{header, Client as ReqwestClient};
use serde_json::{json, Value};
use tracing::{debug, error, info};

// Import our new classification struct and the centralized ApiError
use crate::api::error::ApiError;
use crate::llm::classification::Classification;

// Import extracted modules
pub mod config;
pub mod responses;
pub mod streaming;
pub mod embedding;

// Re-export types for external use
pub use config::{ClientConfig, ModelConfig};
pub use responses::{ResponseOutput, extract_text_from_responses, normalize_verbosity, normalize_reasoning_effort};
pub use streaming::{ResponseStream, sse_json_stream, StreamProcessor};
pub use embedding::{EmbeddingClient, EmbeddingModel, EmbeddingUtils};

/// Main OpenAI client with refactored architecture
pub struct OpenAIClient {
    client: ReqwestClient,
    config: ClientConfig,
    embedding_client: EmbeddingClient,
}

impl OpenAIClient {
    /// Create new OpenAI client from environment configuration
    pub fn new() -> Result<Arc<Self>> {
        let config = ClientConfig::from_env()?;
        config.validate()?;

        info!(
            "Initializing OpenAI client: model={}, verbosity={}, reasoning={}, max_tokens={}",
            config.model(), config.verbosity(), config.reasoning_effort(), config.max_output_tokens()
        );

        let embedding_client = EmbeddingClient::new(config.clone());

        Ok(Arc::new(Self {
            client: ReqwestClient::new(),
            config,
            embedding_client,
        }))
    }

    /// Create client with custom configuration
    pub fn with_config(config: ClientConfig) -> Result<Arc<Self>> {
        config.validate()?;
        
        let embedding_client = EmbeddingClient::new(config.clone());

        Ok(Arc::new(Self {
            client: ReqwestClient::new(),
            config,
            embedding_client,
        }))
    }

    // Configuration getters (preserved for compatibility)
    pub fn model(&self) -> &str {
        self.config.model()
    }

    pub fn verbosity(&self) -> &str {
        self.config.verbosity()
    }

    pub fn reasoning_effort(&self) -> &str {
        self.config.reasoning_effort()
    }

    pub fn max_output_tokens(&self) -> usize {
        self.config.max_output_tokens()
    }

    /// Generate a response using the GPT-5 Responses API (non-streaming)
    pub async fn generate_response(
        &self,
        user_text: &str,
        system_prompt: Option<&str>,
        request_structured: bool,
    ) -> Result<ResponseOutput> {
        let request_body = responses::create_request_body(
            user_text,
            system_prompt,
            self.config.model(),
            self.config.verbosity(),
            self.config.reasoning_effort(),
            self.config.max_output_tokens(),
            request_structured,
        );

        debug!("Sending request to GPT-5 Responses API (non-streaming)");
        let response_value = self.post_response(request_body).await?;

        // Validate and extract response
        responses::validate_response(&response_value)?;
        
        let text_content = responses::extract_text_from_responses(&response_value)
            .ok_or_else(|| {
                anyhow::anyhow!("Failed to extract text from API response")
            })?;

        Ok(ResponseOutput::with_raw(text_content, response_value))
    }

    /// Stream a response using the GPT-5 Responses API (SSE)
    pub async fn stream_response(&self, body: Value) -> Result<ResponseStream> {
        self.post_response_stream(body).await
    }

    /// Conversation summarization (preserved for compatibility)
    pub async fn summarize_conversation(
        &self,
        prompt: &str,
        max_output_tokens: usize,
    ) -> Result<String> {
        let body = json!({
            "model": self.config.model(),
            "input": [{
                "role": "user",
                "content": [{
                    "type": "input_text",
                    "text": prompt
                }]
            }],
            "verbosity": self.config.verbosity(),
            "reasoning_effort": self.config.reasoning_effort(),
            "max_output_tokens": max_output_tokens,
            "temperature": 0.3
        });

        let response = self.post_response(body).await?;
        responses::extract_text_from_responses(&response)
            .ok_or_else(|| anyhow::anyhow!("Failed to extract summarization response"))
    }

    /// Classifies text using the chat completions API with JSON mode.
    pub async fn classify_text(&self, text: &str) -> Result<Classification> {
        let system_prompt = r#"
            You are an expert at analyzing text to extract structured metadata.
            Analyze the following message and output a JSON object with the fields:
            is_code, lang, topics, and salience.

            - is_code: boolean - True if the message is primarily code, false otherwise.
            - lang: string - If is_code is true, specify the programming language (e.g., "rust", "python"). Otherwise, use "natural".
            - topics: array of strings - A list of keywords or domains that describe the content (e.g., ["git", "error_handling"]).
            - salience: float - A score from 0.0 to 1.0 indicating the importance of the message for future context. 1.0 is most important.
        "#;

        let request_body = json!({
            "model": "gpt-4o",
            "messages": [
                { "role": "system", "content": system_prompt },
                { "role": "user", "content": text }
            ],
            "response_format": { "type": "json_object" }
        });

        let response = self
            .client
            .post(&format!("{}/v1/chat/completions", &self.config.base_url()))
            .header(header::AUTHORIZATION, format!("Bearer {}", self.config.api_key()))
            .header(header::CONTENT_TYPE, "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(|e| ApiError::internal(format!("LLM request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(ApiError::internal(format!(
                "Classification API request failed with status {}: {}",
                status, error_text
            )).into());
        }

        let response_data: Value = response.json().await
            .map_err(|e| ApiError::internal(format!("Failed to parse JSON from LLM response: {}", e)))?;

        if let Some(content) = response_data["choices"][0]["message"]["content"].as_str() {
            serde_json::from_str::<Classification>(content)
                .map_err(|e| {
                    error!("Failed to parse classification from LLM content: {}", e);
                    ApiError::internal("LLM returned malformed classification JSON").into()
                })
        } else {
            Err(ApiError::internal("No content in classification response from LLM").into())
        }
    }


    /// Raw HTTP POST to the Responses API - Made public for ResponsesManager
    pub async fn post_response(&self, body: Value) -> Result<Value> {
        let response = self
            .client
            .post(&format!("{}/responses", &self.config.base_url()))
            .header(header::AUTHORIZATION, format!("Bearer {}", self.config.api_key()))
            .header(header::CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("API request failed with status {}: {}", status, error_text));
        }

        let response_data: Value = response.json().await?;
        Ok(response_data)
    }

    /// Raw HTTP POST to streaming Responses API - Made public for ResponsesManager
    pub async fn post_response_stream(&self, body: Value) -> Result<ResponseStream> {
        streaming::create_sse_stream(&self.client, &self.config, body).await
    }

    /// Stream responses (preserved for compatibility)
    async fn post_response_stream_internal(&self, body: Value) -> Result<ResponseStream> {
        let req = self
            .client
            .post(&format!("{}/v1/responses", self.config.base_url()))
            .header(header::AUTHORIZATION, format!("Bearer {}", self.config.api_key()))
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ACCEPT, "text/event-stream")
            .json(&body);

        let resp = req.send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let error_text = resp.text().await.unwrap_or_else(|_| "<no body>".into());
            return Err(anyhow::anyhow!("OpenAI API error ({}): {}", status, error_text));
        }

        let bytes_stream = resp.bytes_stream();
        let stream = streaming::sse_json_stream(bytes_stream);
        Ok(Box::pin(stream))
    }

    /// Generic request builder (preserved for compatibility)
    pub fn request(&self, method: reqwest::Method, endpoint: &str) -> reqwest::RequestBuilder {
        self.client
            .request(method, format!("{}/v1/{}", self.config.base_url(), endpoint))
            .header(header::AUTHORIZATION, format!("Bearer {}", self.config.api_key()))
            .header(header::CONTENT_TYPE, "application/json")
    }

    /// Multipart request builder (preserved for compatibility)
    pub fn request_multipart(&self, endpoint: &str) -> reqwest::RequestBuilder {
        self.client
            .post(format!("{}/v1/{}", self.config.base_url(), endpoint))
            .header(header::AUTHORIZATION, format!("Bearer {}", self.config.api_key()))
    }

    /// Get embeddings for text - Fixed method name
    pub async fn get_embeddings(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        // Handle multiple texts by calling get_embedding for each
        let mut results = Vec::new();
        for text in texts {
            let embedding = self.embedding_client.get_embedding(text).await?;
            results.push(embedding);
        }
        Ok(results)
    }

    /// Get single embedding for text
    pub async fn get_embedding(&self, text: &str) -> Result<Vec<f32>> {
        self.embedding_client.get_embedding(text).await
    }

    /// Get reference to embedded configuration
    pub fn config(&self) -> &ClientConfig {
        &self.config
    }

    /// Get reference to embedding client
    pub fn embedding_client(&self) -> &EmbeddingClient {
        &self.embedding_client
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        // Test with environment variables
        std::env::set_var("OPENAI_API_KEY", "test-key");
        
        let client_result = OpenAIClient::new();
        assert!(client_result.is_ok());
        
        let client = client_result.unwrap();
        assert_eq!(client.model(), "gpt-4o");
    }
}
