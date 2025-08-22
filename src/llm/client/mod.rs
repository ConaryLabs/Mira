// src/llm/client/mod.rs
// Refactored OpenAI Client - Main interface module
// Reduced from ~650-700 lines to ~150 lines by extracting:
// - config.rs: Configuration management
// - responses.rs: Response processing and text extraction
// - streaming.rs: SSE parsing and streaming logic
// - embedding.rs: Embedding operations

use std::sync::Arc;

use anyhow::Result;
use reqwest::{header, Client as ReqwestClient};
use serde_json::{json, Value};
use tracing::{debug, info};

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
            "🚀 Initializing OpenAI client (model={}, verbosity={}, reasoning={}, max_tokens={})",
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

        debug!("📤 Sending request to GPT-5 Responses API (non-streaming)");
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
                "content": [{ "type": "input_text", "text": prompt }]
            }],
            "text": { "verbosity": "low" },
            "reasoning": { "effort": "minimal" },
            "max_output_tokens": max_output_tokens
        });

        debug!("📤 Sending summarization request to GPT-5 Responses API");
        let response_value = self.post_response(body).await?;
        
        responses::extract_text_from_responses(&response_value)
            .ok_or_else(|| anyhow::anyhow!("Failed to extract summary from API response"))
    }

    /// Get embedding for text (delegated to embedding client)
    pub async fn get_embedding(&self, text: &str) -> Result<Vec<f32>> {
        self.embedding_client.get_embedding(text).await
    }

    /// Get embeddings for multiple texts in batch
    pub async fn get_embeddings_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        self.embedding_client.get_embeddings_batch(texts).await
    }

    // === HTTP Helper Methods (Internal) ===

    /// Post request to Responses API (non-streaming)
    pub async fn post_response(&self, body: Value) -> Result<Value> {
        let response = self
            .client
            .post(format!("{}/v1/responses", self.config.base_url()))
            .header(header::AUTHORIZATION, format!("Bearer {}", self.config.api_key()))
            .header(header::CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|_| "<no body>".into());
            return Err(anyhow::anyhow!("OpenAI API error ({}): {}", status, error_text));
        }

        response.json().await.map_err(Into::into)
    }

    /// Post streaming request to Responses API (SSE)
    pub async fn post_response_stream(&self, body: Value) -> Result<ResponseStream> {
        let req = self.client
            .post(format!("{}/v1/responses", self.config.base_url()))
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

    /// Get reference to embedded configuration
    pub fn config(&self) -> &ClientConfig {
        &self.config
    }

    /// Get reference to embedding client
    pub fn embedding_client(&self) -> &EmbeddingClient {
        &self.embedding_client
    }
}

// Re-export the main client type for compatibility - but avoid name conflict
// Note: There's already a simple_chat method in src/llm/chat.rs, so we won't alias as Client

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
        assert_eq!(client.model(), "gpt-5"); // Default model
    }

    #[test]
    fn test_client_with_custom_config() {
        let config = ClientConfig::new(
            "test-key".to_string(),
            "https://api.openai.com".to_string(),
            "gpt-4".to_string(),
            "high".to_string(),
            "low".to_string(),
            2000,
        );

        let client_result = OpenAIClient::with_config(config);
        assert!(client_result.is_ok());
        
        let client = client_result.unwrap();
        assert_eq!(client.model(), "gpt-4");
        assert_eq!(client.verbosity(), "high");
        assert_eq!(client.reasoning_effort(), "low");
        assert_eq!(client.max_output_tokens(), 2000);
    }

    #[test]
    fn test_config_validation() {
        let invalid_config = ClientConfig::new(
            "".to_string(), // Empty API key
            "https://api.openai.com".to_string(),
            "gpt-5".to_string(),
            "medium".to_string(),
            "medium".to_string(),
            1000,
        );

        let client_result = OpenAIClient::with_config(invalid_config);
        assert!(client_result.is_err());
    }

    #[tokio::test]
    async fn test_request_body_creation() {
        std::env::set_var("OPENAI_API_KEY", "test-key");
        let client = OpenAIClient::new().unwrap();

        let body = responses::create_request_body(
            "Hello",
            Some("You are helpful"),
            client.model(),
            client.verbosity(),
            client.reasoning_effort(),
            client.max_output_tokens(),
            false
        );

        assert_eq!(body["model"], client.model());
        assert_eq!(body["input"].as_array().unwrap().len(), 2); // system + user
    }
}
