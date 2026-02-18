// crates/mira-server/src/llm/http_client.rs
// Shared HTTP client configuration for all LLM providers

use anyhow::{Result, anyhow};
use reqwest::Client;
use std::time::Duration;
use tracing::warn;

/// Default maximum retry attempts for transient failures
const DEFAULT_MAX_ATTEMPTS: u32 = 3;
/// Default base backoff duration between retries (doubles each attempt)
const DEFAULT_BASE_BACKOFF_SECS: u64 = 1;
/// Default request timeout when creating from an existing client
const DEFAULT_REQUEST_TIMEOUT_SECS: u64 = 300;
/// Default connect timeout when creating from an existing client
const DEFAULT_CONNECT_TIMEOUT_SECS: u64 = 30;

/// Shared HTTP client configuration for all LLM providers
pub struct LlmHttpClient {
    client: Client,
    pub request_timeout: Duration,
    pub connect_timeout: Duration,
    pub max_attempts: u32,
    pub base_backoff: Duration,
}

impl LlmHttpClient {
    pub fn new(request_timeout: Duration, connect_timeout: Duration) -> Self {
        let client = Client::builder()
            .timeout(request_timeout)
            .connect_timeout(connect_timeout)
            .build()
            .unwrap_or_else(|_| Client::new());

        Self {
            client,
            request_timeout,
            connect_timeout,
            max_attempts: DEFAULT_MAX_ATTEMPTS,
            base_backoff: Duration::from_secs(DEFAULT_BASE_BACKOFF_SECS),
        }
    }

    /// Create from an existing reqwest::Client
    pub fn from_client(client: Client) -> Self {
        Self {
            client,
            request_timeout: Duration::from_secs(DEFAULT_REQUEST_TIMEOUT_SECS),
            connect_timeout: Duration::from_secs(DEFAULT_CONNECT_TIMEOUT_SECS),
            max_attempts: DEFAULT_MAX_ATTEMPTS,
            base_backoff: Duration::from_secs(DEFAULT_BASE_BACKOFF_SECS),
        }
    }

    pub fn inner(&self) -> &Client {
        &self.client
    }

    /// Execute HTTP request with retry logic using Bearer auth
    /// Returns the response body as text on success
    pub async fn execute_with_retry(
        &self,
        request_id: &str,
        url: &str,
        api_key: &str,
        body: String,
    ) -> Result<String> {
        self.execute_request_with_retry(request_id, body, |client, body| {
            client
                .post(url)
                .header("Authorization", format!("Bearer {}", api_key))
                .header("Content-Type", "application/json")
                .body(body)
        })
        .await
    }

    /// Execute HTTP request with retry logic using a custom request builder.
    ///
    /// The `build_request` closure is called on each attempt with the reqwest Client
    /// and the request body, allowing callers to customize URL, headers, and auth.
    pub async fn execute_request_with_retry<F>(
        &self,
        request_id: &str,
        body: String,
        build_request: F,
    ) -> Result<String>
    where
        F: Fn(&Client, String) -> reqwest::RequestBuilder,
    {
        let mut attempts = 0;
        let mut backoff = self.base_backoff;

        loop {
            let response_result = build_request(&self.client, body.clone()).send().await;

            match response_result {
                Ok(response) => {
                    let status = response.status();
                    if !status.is_success() {
                        let error_body = response.text().await.unwrap_or_default();

                        // Check for transient errors
                        if attempts < self.max_attempts
                            && (status.as_u16() == 429 || status.is_server_error())
                        {
                            warn!(
                                request_id = %request_id,
                                status = %status,
                                error = %error_body,
                                "Transient error, retrying in {:?}...",
                                backoff
                            );
                            tokio::time::sleep(backoff).await;
                            attempts += 1;
                            backoff *= 2;
                            continue;
                        }

                        return Err(anyhow!("API error {}: {}", status, error_body));
                    }

                    return Ok(response.text().await?);
                }
                Err(e) => {
                    // Only retry on connection/timeout errors (safe to retry)
                    // Don't retry on other errors (request may have been processed)
                    if attempts < self.max_attempts && (e.is_connect() || e.is_timeout()) {
                        warn!(
                            request_id = %request_id,
                            error = %e,
                            "Request failed (connect/timeout), retrying in {:?}...",
                            backoff
                        );
                        tokio::time::sleep(backoff).await;
                        attempts += 1;
                        backoff *= 2;
                        continue;
                    }
                    return Err(anyhow!("Request failed after retries: {}", e));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // Construction
    // ========================================================================

    #[test]
    fn test_client_creation() {
        let client = LlmHttpClient::new(Duration::from_secs(10), Duration::from_secs(5));
        assert_eq!(client.max_attempts, DEFAULT_MAX_ATTEMPTS);
        assert_eq!(client.base_backoff, Duration::from_secs(DEFAULT_BASE_BACKOFF_SECS));
        assert_eq!(client.request_timeout, Duration::from_secs(10));
        assert_eq!(client.connect_timeout, Duration::from_secs(5));
    }

    #[test]
    fn test_from_client() {
        let reqwest_client = Client::new();
        let client = LlmHttpClient::from_client(reqwest_client);
        assert_eq!(client.max_attempts, DEFAULT_MAX_ATTEMPTS);
        assert_eq!(client.base_backoff, Duration::from_secs(DEFAULT_BASE_BACKOFF_SECS));
        assert_eq!(client.request_timeout, Duration::from_secs(DEFAULT_REQUEST_TIMEOUT_SECS));
        assert_eq!(client.connect_timeout, Duration::from_secs(DEFAULT_CONNECT_TIMEOUT_SECS));
    }

    #[test]
    fn test_inner_returns_client() {
        let client = LlmHttpClient::new(Duration::from_secs(10), Duration::from_secs(5));
        let _inner = client.inner();
    }

    #[test]
    fn test_default_constants() {
        assert_eq!(DEFAULT_MAX_ATTEMPTS, 3);
        assert_eq!(DEFAULT_BASE_BACKOFF_SECS, 1);
        assert_eq!(DEFAULT_REQUEST_TIMEOUT_SECS, 300);
        assert_eq!(DEFAULT_CONNECT_TIMEOUT_SECS, 30);
    }

    // ========================================================================
    // Retry behavior (requires tokio + actual HTTP)
    // ========================================================================

    #[tokio::test]
    async fn test_execute_with_retry_connection_refused() {
        let client = LlmHttpClient {
            client: Client::new(),
            request_timeout: Duration::from_millis(500),
            connect_timeout: Duration::from_millis(200),
            max_attempts: 1, // Only 1 retry to keep test fast
            base_backoff: Duration::from_millis(10),
        };
        let result = client
            .execute_with_retry("test", "http://127.0.0.1:1", "key", "{}".into())
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("failed") || err.contains("error") || err.contains("connect"),
            "Expected connection error, got: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_execute_request_with_retry_custom_builder() {
        let client = LlmHttpClient {
            client: Client::new(),
            request_timeout: Duration::from_millis(500),
            connect_timeout: Duration::from_millis(200),
            max_attempts: 0, // No retries
            base_backoff: Duration::from_millis(10),
        };
        let result = client
            .execute_request_with_retry("test", "{}".into(), |c, body| {
                c.post("http://127.0.0.1:1")
                    .header("Content-Type", "application/json")
                    .body(body)
            })
            .await;
        assert!(result.is_err());
    }
}
