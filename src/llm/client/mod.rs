// src/llm/client/mod.rs

use std::sync::Arc;
use std::time::Duration;
use anyhow::Result;
use reqwest::{header, Client};
use serde_json::Value;
use tracing::{debug, error, info, warn};

use crate::llm::structured::CompleteResponse;

pub mod config;
pub mod embedding;
pub mod responses;

use config::ClientConfig;
use embedding::EmbeddingClient;

pub struct OpenAIClient {
    client: Client,
    config: ClientConfig,
    rate_limiter: Arc<tokio::sync::Semaphore>,
    embedding_client: EmbeddingClient,
}

impl Clone for OpenAIClient {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
            config: self.config.clone(),
            rate_limiter: self.rate_limiter.clone(),
            embedding_client: EmbeddingClient::new(self.config.clone()),
        }
    }
}

impl OpenAIClient {
    pub fn new(config: ClientConfig) -> Result<Self> {
        config.validate()?;
        
        let client = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()?;
        
        let rate_limiter = Arc::new(tokio::sync::Semaphore::new(10));
        let embedding_client = EmbeddingClient::new(config.clone());

        Ok(Self {
            client,
            config,
            rate_limiter,
            embedding_client,
        })
    }

    // Delegate to the actual embedding client implementation
    pub async fn get_embedding(&self, text: &str) -> Result<Vec<f32>> {
        self.embedding_client.get_embedding(text).await
    }

    // Return reference to embedding client for existing code
    pub fn embedding_client(&self) -> &EmbeddingClient {
        &self.embedding_client
    }

    // FIXED: Now uses Claude Messages API format
    pub async fn summarize_conversation(&self, prompt: &str, _max_tokens: usize) -> Result<String> {
        debug!("Summarization request for prompt length: {} characters", prompt.len());
        
        // Build Claude Messages API request
        let request_body = serde_json::json!({
            "model": self.config.model,
            "max_tokens": self.config.max_output_tokens,
            "messages": [
                {
                    "role": "user",
                    "content": prompt
                }
            ]
        });
        
        let response = self.post_response_with_retry(request_body).await?;
        
        // Extract text from Claude response
        if let Some(content) = response["content"].as_array() {
            for block in content {
                if block["type"] == "text" {
                    if let Some(text) = block["text"].as_str() {
                        return Ok(text.to_string());
                    }
                }
            }
        }
        
        warn!("Could not extract text from Claude response");
        Err(anyhow::anyhow!("No text content in Claude response"))
    }

    // FIXED: Now uses Claude Messages API format
    pub async fn generate_response(&self, prompt: &str, _context: Option<&str>, _json: bool) -> Result<String> {
        debug!("Generation request for prompt length: {}", prompt.len());
        
        // Build Claude Messages API request
        let request_body = serde_json::json!({
            "model": self.config.model,
            "max_tokens": self.config.max_output_tokens,
            "messages": [
                {
                    "role": "user",
                    "content": prompt
                }
            ]
        });
        
        let response = self.post_response_with_retry(request_body).await?;
        
        // Extract text from Claude response
        if let Some(content) = response["content"].as_array() {
            for block in content {
                if block["type"] == "text" {
                    if let Some(text) = block["text"].as_str() {
                        return Ok(text.to_string());
                    }
                }
            }
        }
        
        warn!("Could not extract text from Claude response");
        Err(anyhow::anyhow!("No text content in Claude response"))
    }

    pub async fn get_structured_response(
        &self,
        user_message: &str,
        system_prompt: String,
        context_messages: Vec<Value>,
        session_id: &str,
    ) -> Result<CompleteResponse> {
        info!("Requesting structured response for session: {}", session_id);
        
        let start = std::time::Instant::now();
        
        let request_body = crate::llm::structured::processor::build_structured_request(
            user_message,
            system_prompt,
            context_messages,
        )?;
        
        // DEBUG: Log what we're sending
        debug!("🔍 Claude request body: {}", serde_json::to_string_pretty(&request_body).unwrap_or_default());
        
        let raw_response = self.post_response_with_retry(request_body).await?;
        
        // DEBUG LOGGING - Remove after verification
        error!("📥 Claude raw response: {}", serde_json::to_string_pretty(&raw_response).unwrap_or_default());
        
        let latency_ms = start.elapsed().as_millis() as i64;
        
        let metadata = crate::llm::structured::processor::extract_metadata(&raw_response, latency_ms)?;
        let structured = crate::llm::structured::processor::extract_structured_content(&raw_response)?;
        
        crate::llm::structured::validator::validate_response(&structured)?;
        
        info!("Structured response: salience={}, topics={}, tokens={:?}",
              structured.analysis.salience,
              structured.analysis.topics.len(),
              metadata.total_tokens);
        
        Ok(CompleteResponse {
            structured,
            metadata,
            raw_response,
            artifacts: None,
        })
    }

    pub async fn post_response_with_retry(&self, body: Value) -> Result<Value> {
        let max_retries = 3;
        let mut retry_count = 0;
        let mut retry_delay = Duration::from_millis(1000);

        loop {
            retry_count += 1;
            
            let _permit = self.rate_limiter.acquire().await?;
            
            match self.post_response_internal(body.clone()).await {
                Ok(response) => return Ok(response),
                Err(e) => {
                    let error_str = e.to_string();
                    
                    if retry_count < max_retries {
                        warn!(
                            "Request failed (attempt {}/{}), retrying in {:?}: {}", 
                            retry_count, max_retries, retry_delay, error_str
                        );
                        
                        tokio::time::sleep(retry_delay).await;
                        
                        retry_delay = Duration::from_millis(
                            (retry_delay.as_millis() as u64 * 2).min(10000)
                        );
                    } else {
                        error!("Request failed after {} attempts: {}", retry_count, error_str);
                        return Err(e);
                    }
                }
            }
        }
    }

    // UPDATED FOR CLAUDE WITH PROMPT CACHING + CONTEXT MANAGEMENT
    async fn post_response_internal(&self, body: Value) -> Result<Value> {
        let url = format!("{}/v1/messages", &self.config.base_url());
        
        let response = self
            .client
            .post(url)
            .header("x-api-key", self.config.api_key())
            .header("anthropic-version", "2023-06-01")
            .header("anthropic-beta", "prompt-caching-2024-07-31,context-management-2025-06-27")  // NEW: Enable caching + context management
            .header(header::CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            error!("🔥 Claude API error ({} {}): {}", 
                status.as_u16(), status.canonical_reason().unwrap_or("Unknown"), error_text);
            return Err(anyhow::anyhow!("Claude API error ({} {}): {}", 
                status.as_u16(), status.canonical_reason().unwrap_or("Unknown"), error_text));
        }

        let response_data: Value = response.json().await?;
        
        // DEBUG: Log the response (kept at debug level, main logging is in get_structured_response)
        debug!("✅ Claude response received: {}", serde_json::to_string_pretty(&response_data).unwrap_or_default());
        
        Ok(response_data)
    }

    pub async fn post_response(&self, body: Value) -> Result<Value> {
        self.post_response_with_retry(body).await
    }

    pub fn request(&self, method: reqwest::Method, endpoint: &str) -> reqwest::RequestBuilder {
        let url = if endpoint.starts_with("/v1/") {
            format!("{}{}", self.config.base_url(), endpoint)
        } else if endpoint.starts_with("v1/") {
            format!("{}/{}", self.config.base_url(), endpoint)
        } else {
            format!("{}/v1/{}", self.config.base_url(), endpoint)
        };
        
        self.client
            .request(method, url)
            .header("x-api-key", self.config.api_key())
            .header("anthropic-version", "2023-06-01")
            .header(header::CONTENT_TYPE, "application/json")
    }

    pub fn request_multipart(&self, endpoint: &str) -> reqwest::RequestBuilder {
        let url = if endpoint.starts_with("/v1/") {
            format!("{}{}", self.config.base_url(), endpoint)
        } else if endpoint.starts_with("v1/") {
            format!("{}/{}", self.config.base_url(), endpoint)
        } else {
            format!("{}/v1/{}", self.config.base_url(), endpoint)
        };
        
        self.client
            .request(reqwest::Method::POST, url)
            .header("x-api-key", self.config.api_key())
            .header("anthropic-version", "2023-06-01")
    }
}
