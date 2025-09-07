// src/llm/client/mod.rs

use std::sync::Arc;

use anyhow::Result;
use reqwest::{header, Client as ReqwestClient};
use serde_json::{json, Value};
use tracing::{debug, error, info};

// Import our new classification struct and the centralized ApiError
use crate::api::error::ApiError;
use crate::llm::classification::Classification;
use crate::config::CONFIG;

// Import extracted modules
pub mod config;
pub mod responses;
pub mod streaming;
pub mod embedding;

// Re-export types for external use
pub use config::ClientConfig;
pub use responses::{ResponseOutput, extract_text_from_responses};
pub use streaming::ResponseStream;
pub use embedding::EmbeddingClient;

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

    // Configuration getters
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

    /// Conversation summarization
    pub async fn summarize_conversation(
        &self,
        prompt: &str,
        max_output_tokens: usize,
    ) -> Result<String> {
        let body = json!({
            "model": CONFIG.gpt5_model,
            "input": [{
                "role": "user",
                "content": [{
                    "type": "input_text",
                    "text": prompt
                }]
            }],
            "text": {
                "verbosity": CONFIG.get_verbosity_for("summary")
            },
            "parameters": {
                "verbosity": CONFIG.get_verbosity_for("summary"),
                "reasoning_effort": CONFIG.get_reasoning_effort_for("summary"),
                "max_output_tokens": max_output_tokens,
                "temperature": 0.3
            }
        });

        let response = self.post_response(body).await?;
        responses::extract_text_from_responses(&response)
            .ok_or_else(|| anyhow::anyhow!("Failed to extract summarization response"))
    }

    /// Classifies text using GPT-5 Responses API with JSON mode
    pub async fn classify_text(&self, text: &str) -> Result<Classification> {
        info!("🔍 Classifying text with GPT-5 Responses API");
        
        let instructions = r#"
            You are an expert at analyzing text to extract structured metadata.
            Analyze the following message and output a JSON object with the fields:
            is_code, lang, topics, and salience.

            - is_code: boolean - True if the message is primarily code, false otherwise.
            - lang: string - If is_code is true, specify the programming language (e.g., "rust", "python"). Otherwise, use "natural".
            - topics: array of strings - A list of keywords or domains that describe the content (e.g., ["git", "error_handling"]).
            - salience: float - A score from 0.0 to 1.0 indicating the importance of the message for future context. 1.0 is most important.
            
            Be concise and accurate. Use minimal reasoning.
        "#;

        let request_body = json!({
            "model": CONFIG.gpt5_model,
            "input": [{
                "role": "user",
                "content": [{
                    "type": "input_text",
                    "text": text
                }]
            }],
            "instructions": instructions,
            "text": {
                "format": "json_object",
                "verbosity": CONFIG.get_verbosity_for("classification")
            },
            "parameters": {
                "verbosity": CONFIG.get_verbosity_for("classification"),
                "reasoning_effort": CONFIG.get_reasoning_effort_for("classification"),
                "max_output_tokens": CONFIG.get_json_max_tokens()
            }
        });

        debug!("Classification request: model={}, reasoning={}, verbosity={}", 
            CONFIG.gpt5_model, 
            CONFIG.get_reasoning_effort_for("classification"),
            CONFIG.get_verbosity_for("classification")
        );

        let response = self.post_response(request_body).await
            .map_err(|e| ApiError::internal(format!("Classification request failed: {e}")))?;

        let content = responses::extract_text_from_responses(&response)
            .ok_or_else(|| ApiError::internal("No content in classification response"))?;

        serde_json::from_str::<Classification>(&content)
            .map_err(|e| {
                error!("Failed to parse classification JSON: {}\nRaw content: {}", e, content);
                ApiError::internal("LLM returned malformed classification JSON").into()
            })
    }

    /// Raw HTTP POST to the Responses API
    pub async fn post_response(&self, body: Value) -> Result<Value> {
        let response = self
            .client
            .post(format!("{}/v1/responses", &self.config.base_url()))
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

    /// Raw HTTP POST to streaming Responses API
    pub async fn post_response_stream(&self, body: Value) -> Result<ResponseStream> {
        streaming::create_sse_stream(&self.client, &self.config, body).await
    }

    /// Generic request builder - Used by other LLM subsystems
    pub fn request(&self, method: reqwest::Method, endpoint: &str) -> reqwest::RequestBuilder {
        self.client
            .request(method, format!("{}/v1/{}", self.config.base_url(), endpoint))
            .header(header::AUTHORIZATION, format!("Bearer {}", self.config.api_key()))
            .header(header::CONTENT_TYPE, "application/json")
    }

    /// Multipart request builder - Used for file uploads
    pub fn request_multipart(&self, endpoint: &str) -> reqwest::RequestBuilder {
        self.client
            .post(format!("{}/v1/{}", self.config.base_url(), endpoint))
            .header(header::AUTHORIZATION, format!("Bearer {}", self.config.api_key()))
    }

    /// Get embeddings for text
    pub async fn get_embeddings(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        self.embedding_client.get_embeddings_batch(texts).await
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
