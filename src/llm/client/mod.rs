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

    // FIXED: Use the same extraction logic as extract_text_from_responses
    pub async fn summarize_conversation(&self, prompt: &str, max_tokens: usize) -> Result<String> {
        debug!("Summarization request for prompt length: {} characters", prompt.len());
        
        let request_body = serde_json::json!({
            "model": self.config.model,
            "input": prompt,
            "text": {
                "verbosity": "medium"
            }
        });
        
        let response = self.post_response_with_retry(request_body).await?;
        
        // Use the same extraction logic as extract_text_from_responses
        if let Some(text) = responses::extract_text_from_responses(&response) {
            return Ok(text);
        }
        
        // Fallback
        warn!("Could not extract text from response, using fallback");
        Ok("Summary generation failed".to_string())
    }

    // This is probably used by the tools - simple completion as well
    pub async fn generate_response(&self, prompt: &str, _context: Option<&str>, _json: bool) -> Result<String> {
        debug!("Generation request for prompt length: {}", prompt.len());
        
        // Use summarize_conversation for now since it's essentially the same
        self.summarize_conversation(prompt, 2000).await
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
        
        let raw_response = self.post_response_with_retry(request_body).await?;
        
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

    async fn post_response_internal(&self, body: Value) -> Result<Value> {
        let url = format!("{}/v1/responses", &self.config.base_url());
        debug!("Making request to: {}", url);
        
        let response = self
            .client
            .post(url)
            .header(header::AUTHORIZATION, format!("Bearer {}", self.config.api_key()))
            .header(header::CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            error!("OpenAI API error ({} {}): {}", 
                status.as_u16(), status.canonical_reason().unwrap_or("Unknown"), error_text);
            return Err(anyhow::anyhow!("OpenAI API error ({} {}): {}", 
                status.as_u16(), status.canonical_reason().unwrap_or("Unknown"), error_text));
        }

        let response_data: Value = response.json().await?;
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
            .header(header::AUTHORIZATION, format!("Bearer {}", self.config.api_key()))
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
            .header(header::AUTHORIZATION, format!("Bearer {}", self.config.api_key()))
    }
}
