// src/llm/client/mod.rs

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use reqwest::{header, Client as ReqwestClient};
use serde_json::{json, Value};
use tracing::{debug, error, info, warn};

use crate::api::error::ApiError;
use crate::llm::classification::Classification;
use crate::config::CONFIG;

pub mod config;
pub mod responses;
pub mod streaming;
pub mod embedding;

pub use config::ClientConfig;
pub use responses::{ResponseOutput, extract_text_from_responses};
pub use streaming::ResponseStream;
pub use embedding::EmbeddingClient;

// Rate limiting support
use governor::{Quota, RateLimiter as GovRateLimiter, Jitter};
use governor::clock::DefaultClock;
use governor::state::{InMemoryState, NotKeyed};
use std::num::NonZeroU32;

/// Rate limiter for API calls
struct RateLimiter {
    limiter: Arc<GovRateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
    jitter: Jitter,
}

impl RateLimiter {
    fn new(requests_per_minute: u32) -> Result<Self> {
        let quota = Quota::per_minute(
            NonZeroU32::new(requests_per_minute)
                .ok_or_else(|| anyhow::anyhow!("Invalid rate limit"))?
        );
        
        Ok(Self {
            limiter: Arc::new(GovRateLimiter::direct(quota)),
            jitter: Jitter::new(
                Duration::from_millis(10),
                Duration::from_millis(100),
            ),
        })
    }
    
    async fn acquire(&self) -> Result<()> {
        self.limiter.until_ready_with_jitter(self.jitter).await;
        Ok(())
    }
}

/// Main OpenAI client with refactored architecture
pub struct OpenAIClient {
    client: ReqwestClient,
    config: ClientConfig,
    embedding_client: EmbeddingClient,
    rate_limiter: Arc<RateLimiter>,
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

        // Enhanced client with connection pooling for GPT-5 performance
        let client = ReqwestClient::builder()
            .timeout(Duration::from_secs(CONFIG.openai_timeout))
            .pool_max_idle_per_host(10)
            .pool_idle_timeout(Duration::from_secs(90))
            .tcp_keepalive(Duration::from_secs(60))
            .build()?;

        let embedding_client = EmbeddingClient::new(config.clone());
        
        // Create rate limiter based on config
        let rate_limiter = Arc::new(RateLimiter::new(CONFIG.rate_limit_chat as u32)?);

        Ok(Arc::new(Self {
            client,
            config,
            embedding_client,
            rate_limiter,
        }))
    }

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
        let response_value = self.post_response_with_retry(request_body).await?;

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

    /// Conversation summarization with proper text output format
    pub async fn summarize_conversation(
        &self,
        prompt: &str,
        max_output_tokens: usize,
    ) -> Result<String> {
        info!("Generating conversation summary with GPT-5");
        
        // Build the request with LATEST GPT-5 API structure (Sept 2025)
        let body = json!({
            "model": CONFIG.gpt5_model,
            "input": [{
                "role": "user",
                "content": [{
                    "type": "input_text",
                    "text": prompt
                }]
            }],
            "instructions": "Create a concise, factual summary of the conversation. Focus on key points, decisions, and important context.",
            "text": {
                "format": {
                    "type": "text"
                },
                "verbosity": "low"
            },
            "reasoning": {
                "effort": "low"
            },
            "max_output_tokens": max_output_tokens
        });

        debug!("Summarization request: model={}, max_tokens={}", 
            CONFIG.gpt5_model, max_output_tokens);

        let response = self.post_response_with_retry(body).await?;
        
        // Log the response structure for debugging
        if response.get("output").and_then(|o| o.as_array()).map(|a| a.is_empty()).unwrap_or(false) {
            error!("GPT-5 returned empty output array for summarization");
            return Err(anyhow::anyhow!("GPT-5 returned empty output for summarization"));
        }
        
        responses::extract_text_from_responses(&response)
            .ok_or_else(|| {
                error!("Failed to extract text from summarization response. Response: {:?}", response);
                anyhow::anyhow!("Failed to extract summarization response")
            })
    }

    /// Classifies text using GPT-5 Responses API with JSON mode
    pub async fn classify_text(&self, text: &str) -> Result<Classification> {
        info!("Classifying text with GPT-5 Responses API");
        
        let instructions = r#"You are an expert at analyzing text to extract structured metadata.
Analyze the following message and return your response as a JSON object.

The JSON response must include these fields:
- is_code: boolean - True if the message is primarily code, false otherwise.
- lang: string - If is_code is true, specify the programming language (e.g., "rust", "python"). Otherwise, use "natural".
- topics: array of strings - A list of keywords or domains that describe the content (e.g., ["git", "error_handling"]).
- salience: float - A score from 0.0 to 1.0 indicating the importance of the message for future context. 1.0 is most important.

Be concise and accurate. Output your analysis as valid JSON only."#;

        let request_body = json!({
            "model": CONFIG.gpt5_model,
            "input": [{
                "role": "user", 
                "content": [{
                    "type": "input_text",
                    "text": format!("Analyze this text and return a JSON classification:\n\n{}", text)
                }]
            }],
            "instructions": instructions,
            "text": {
                "format": {
                    "type": "json_object"
                }
            },
            "max_output_tokens": CONFIG.get_json_max_tokens()
        });

        debug!("Classification request: model={}, max_tokens={}", 
            CONFIG.gpt5_model, 
            CONFIG.get_json_max_tokens()
        );

        let response = self.post_response_with_retry(request_body).await
            .map_err(|e| ApiError::internal(format!("Classification request failed: {e}")))?;

        let content = responses::extract_text_from_responses(&response)
            .ok_or_else(|| ApiError::internal("No content in classification response"))?;

        serde_json::from_str::<Classification>(&content)
            .map_err(|e| {
                error!("Failed to parse classification JSON: {}\nRaw content: {}", e, content);
                ApiError::internal("LLM returned malformed classification JSON").into()
            })
    }

    /// Raw HTTP POST to the Responses API with retry logic
    pub async fn post_response_with_retry(&self, body: Value) -> Result<Value> {
        let max_retries = CONFIG.api_max_retries;
        let mut retry_count = 0;
        let mut retry_delay = Duration::from_millis(CONFIG.api_retry_delay_ms);

        loop {
            // Apply rate limiting
            self.rate_limiter.acquire().await?;

            match self.post_response_internal(body.clone()).await {
                Ok(response) => return Ok(response),
                Err(e) => {
                    let error_str = e.to_string();
                    
                    // Check if error is retryable
                    let is_retryable = error_str.contains("429") || 
                                      error_str.contains("500") || 
                                      error_str.contains("502") || 
                                      error_str.contains("503") ||
                                      error_str.contains("504");
                    
                    if is_retryable && retry_count < max_retries {
                        retry_count += 1;
                        warn!(
                            "Request failed (attempt {}/{}), retrying in {:?}: {}", 
                            retry_count, max_retries, retry_delay, error_str
                        );
                        
                        tokio::time::sleep(retry_delay).await;
                        
                        // Exponential backoff
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

    /// Internal POST method without retry
    async fn post_response_internal(&self, body: Value) -> Result<Value> {
        let response = self
            .client
            .post(format!("{}/openai/v1/responses", &self.config.base_url()))
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

    /// Raw HTTP POST to the Responses API (backward compatibility)
    pub async fn post_response(&self, body: Value) -> Result<Value> {
        self.post_response_with_retry(body).await
    }

    /// Raw HTTP POST to streaming Responses API
    pub async fn post_response_stream(&self, body: Value) -> Result<ResponseStream> {
        // Apply rate limiting before streaming
        self.rate_limiter.acquire().await?;
        streaming::create_sse_stream(&self.client, &self.config, body).await
    }

    /// Generic request builder - Fixed to not double /v1
    pub fn request(&self, method: reqwest::Method, endpoint: &str) -> reqwest::RequestBuilder {
        self.client
            .request(method, format!("{}/{}", self.config.base_url(), endpoint))
            .header(header::AUTHORIZATION, format!("Bearer {}", self.config.api_key()))
            .header(header::CONTENT_TYPE, "application/json")
    }

    /// Multipart request builder - Fixed to not double /v1
    pub fn request_multipart(&self, endpoint: &str) -> reqwest::RequestBuilder {
        self.client
            .post(format!("{}/{}", self.config.base_url(), endpoint))
            .header(header::AUTHORIZATION, format!("Bearer {}", self.config.api_key()))
    }

    /// Get embeddings for text with automatic retry
    pub async fn get_embeddings(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let max_retries = CONFIG.api_max_retries;
        let mut retry_count = 0;
        
        loop {
            match self.embedding_client.get_embeddings_batch(texts).await {
                Ok(embeddings) => return Ok(embeddings),
                Err(e) if retry_count < max_retries => {
                    retry_count += 1;
                    let delay = Duration::from_millis(CONFIG.api_retry_delay_ms * retry_count as u64);
                    warn!("Embedding request failed (attempt {}/{}), retrying in {:?}", 
                        retry_count, max_retries, delay);
                    tokio::time::sleep(delay).await;
                }
                Err(e) => return Err(e),
            }
        }
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
