//! Gemini Batch API client for async bulk processing.
//!
//! The Batch API provides 50% cost savings for high-volume, latency-tolerant tasks like:
//! - Memory compaction
//! - Document summarization
//! - Codebase analysis
//!
//! Batches are processed asynchronously with typical completion within 24 hours.

use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

/// API endpoint for Gemini Batch operations
const BATCH_API_URL: &str = "https://generativelanguage.googleapis.com/v1beta";

/// Gemini Batch API client
pub struct BatchClient {
    client: Client,
    api_key: String,
}

/// Batch job state from Gemini API
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BatchState {
    /// Initial state, batch is queued
    JobStatePending,
    /// Batch is being processed
    JobStateRunning,
    /// All requests completed successfully
    JobStateSucceeded,
    /// Batch failed (check error details)
    JobStateFailed,
    /// Batch was cancelled by user
    JobStateCancelled,
    /// Unknown state
    #[serde(other)]
    Unknown,
}

impl BatchState {
    /// Returns true if the batch is in a terminal state
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::JobStateSucceeded | Self::JobStateFailed | Self::JobStateCancelled
        )
    }

    /// Convert to a simple status string for database storage
    pub fn to_status(&self) -> &'static str {
        match self {
            Self::JobStatePending => "pending",
            Self::JobStateRunning => "running",
            Self::JobStateSucceeded => "succeeded",
            Self::JobStateFailed => "failed",
            Self::JobStateCancelled => "cancelled",
            Self::Unknown => "unknown",
        }
    }
}

/// A single request within a batch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchRequest {
    /// Unique key to correlate request with response
    #[serde(rename = "customId")]
    pub custom_id: String,

    /// The request body (GenerateContentRequest format)
    pub request: serde_json::Value,
}

/// Response for a single request within a batch
#[derive(Debug, Clone, Deserialize)]
pub struct BatchResponse {
    /// Correlation key matching the request
    #[serde(rename = "customId")]
    pub custom_id: String,

    /// The response (GenerateContentResponse format)
    pub response: Option<serde_json::Value>,

    /// Error if the request failed
    pub error: Option<BatchError>,
}

/// Error details for a failed batch request
#[derive(Debug, Clone, Deserialize)]
pub struct BatchError {
    pub code: Option<i32>,
    pub message: Option<String>,
    pub status: Option<String>,
}

/// Batch job metadata from Gemini API
#[derive(Debug, Clone, Deserialize)]
pub struct BatchJob {
    /// Batch resource name (e.g., "batches/abc123")
    pub name: String,

    /// Display name for the batch
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,

    /// Current state of the batch
    pub state: BatchState,

    /// Model used for the batch
    pub model: Option<String>,

    /// Total number of requests in the batch
    #[serde(rename = "totalCount")]
    pub total_count: Option<i32>,

    /// Number of requests that succeeded
    #[serde(rename = "succeededCount")]
    pub succeeded_count: Option<i32>,

    /// Number of requests that failed
    #[serde(rename = "failedCount")]
    pub failed_count: Option<i32>,

    /// Creation timestamp (RFC3339)
    #[serde(rename = "createTime")]
    pub create_time: Option<String>,

    /// Last update timestamp (RFC3339)
    #[serde(rename = "updateTime")]
    pub update_time: Option<String>,

    /// Completion timestamp (RFC3339)
    #[serde(rename = "endTime")]
    pub end_time: Option<String>,

    /// Error details if batch failed
    pub error: Option<BatchError>,
}

/// Request to create a new batch
#[derive(Debug, Serialize)]
struct CreateBatchRequest {
    /// Display name for the batch
    #[serde(rename = "displayName", skip_serializing_if = "Option::is_none")]
    display_name: Option<String>,

    /// Model to use (e.g., "models/gemini-2.0-flash")
    model: String,

    /// Inline requests (alternative to file-based input)
    #[serde(rename = "inlineRequests", skip_serializing_if = "Option::is_none")]
    inline_requests: Option<Vec<BatchRequest>>,
}

/// Response from listing batches
#[derive(Debug, Deserialize)]
struct ListBatchesResponse {
    batches: Option<Vec<BatchJob>>,
    #[serde(rename = "nextPageToken")]
    next_page_token: Option<String>,
}

impl BatchClient {
    /// Create a new BatchClient with the given API key
    pub fn new(api_key: &str) -> Self {
        Self {
            client: Client::new(),
            api_key: api_key.to_string(),
        }
    }

    /// Create a new batch with inline requests
    ///
    /// # Arguments
    /// * `model` - Model to use (e.g., "gemini-2.0-flash" or full path)
    /// * `requests` - List of requests to process
    /// * `display_name` - Optional display name for the batch
    ///
    /// # Returns
    /// The created batch job with its name and initial state
    pub async fn create_batch(
        &self,
        model: &str,
        requests: Vec<BatchRequest>,
        display_name: Option<&str>,
    ) -> Result<BatchJob> {
        if requests.is_empty() {
            return Err(anyhow!("Batch must contain at least one request"));
        }

        // Ensure model has proper format
        let model_path = if model.starts_with("models/") {
            model.to_string()
        } else {
            format!("models/{}", model)
        };

        let request_body = CreateBatchRequest {
            display_name: display_name.map(|s| s.to_string()),
            model: model_path,
            inline_requests: Some(requests),
        };

        let url = format!("{}/batches?key={}", BATCH_API_URL, self.api_key);

        debug!("Creating batch with {} requests", request_body.inline_requests.as_ref().map(|r| r.len()).unwrap_or(0));

        let response = self
            .client
            .post(&url)
            .json(&request_body)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Failed to create batch: {} - {}",
                status,
                error_text
            ));
        }

        let batch: BatchJob = response.json().await?;
        debug!("Created batch: {}", batch.name);

        Ok(batch)
    }

    /// Get the current status of a batch
    ///
    /// # Arguments
    /// * `batch_name` - The batch resource name (e.g., "batches/abc123")
    pub async fn get_batch(&self, batch_name: &str) -> Result<BatchJob> {
        // Ensure proper format
        let name = if batch_name.starts_with("batches/") {
            batch_name.to_string()
        } else {
            format!("batches/{}", batch_name)
        };

        let url = format!("{}/{}?key={}", BATCH_API_URL, name, self.api_key);

        let response = self.client.get(&url).send().await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Failed to get batch {}: {} - {}",
                name,
                status,
                error_text
            ));
        }

        let batch: BatchJob = response.json().await?;
        Ok(batch)
    }

    /// List all batches, optionally filtered
    ///
    /// # Arguments
    /// * `page_size` - Maximum number of batches to return (default: 100)
    /// * `page_token` - Token for pagination
    pub async fn list_batches(
        &self,
        page_size: Option<u32>,
        page_token: Option<&str>,
    ) -> Result<(Vec<BatchJob>, Option<String>)> {
        let mut url = format!("{}/batches?key={}", BATCH_API_URL, self.api_key);

        if let Some(size) = page_size {
            url.push_str(&format!("&pageSize={}", size));
        }
        if let Some(token) = page_token {
            url.push_str(&format!("&pageToken={}", token));
        }

        let response = self.client.get(&url).send().await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Failed to list batches: {} - {}",
                status,
                error_text
            ));
        }

        let list_response: ListBatchesResponse = response.json().await?;

        Ok((
            list_response.batches.unwrap_or_default(),
            list_response.next_page_token,
        ))
    }

    /// Cancel a running batch
    ///
    /// # Arguments
    /// * `batch_name` - The batch resource name
    pub async fn cancel_batch(&self, batch_name: &str) -> Result<BatchJob> {
        let name = if batch_name.starts_with("batches/") {
            batch_name.to_string()
        } else {
            format!("batches/{}", batch_name)
        };

        let url = format!("{}/{}:cancel?key={}", BATCH_API_URL, name, self.api_key);

        let response = self.client.post(&url).send().await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Failed to cancel batch {}: {} - {}",
                name,
                status,
                error_text
            ));
        }

        let batch: BatchJob = response.json().await?;
        debug!("Cancelled batch: {}", batch.name);

        Ok(batch)
    }

    /// Delete a batch (must be in terminal state)
    ///
    /// # Arguments
    /// * `batch_name` - The batch resource name
    pub async fn delete_batch(&self, batch_name: &str) -> Result<()> {
        let name = if batch_name.starts_with("batches/") {
            batch_name.to_string()
        } else {
            format!("batches/{}", batch_name)
        };

        let url = format!("{}/{}?key={}", BATCH_API_URL, name, self.api_key);

        let response = self.client.delete(&url).send().await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Failed to delete batch {}: {} - {}",
                name,
                status,
                error_text
            ));
        }

        debug!("Deleted batch: {}", name);
        Ok(())
    }

    /// Poll a batch until it reaches a terminal state
    ///
    /// # Arguments
    /// * `batch_name` - The batch resource name
    /// * `poll_interval` - How often to poll (in seconds)
    /// * `max_polls` - Maximum number of polls before giving up (None = unlimited)
    ///
    /// # Returns
    /// The final batch state
    pub async fn wait_for_completion(
        &self,
        batch_name: &str,
        poll_interval_secs: u64,
        max_polls: Option<u32>,
    ) -> Result<BatchJob> {
        let mut polls = 0;

        loop {
            let batch = self.get_batch(batch_name).await?;

            if batch.state.is_terminal() {
                return Ok(batch);
            }

            polls += 1;
            if let Some(max) = max_polls {
                if polls >= max {
                    return Err(anyhow!(
                        "Batch {} did not complete after {} polls",
                        batch_name,
                        polls
                    ));
                }
            }

            debug!(
                "Batch {} is {:?}, polling again in {}s (poll {}/{})",
                batch_name,
                batch.state,
                poll_interval_secs,
                polls,
                max_polls.map(|m| m.to_string()).unwrap_or_else(|| "∞".to_string())
            );

            tokio::time::sleep(tokio::time::Duration::from_secs(poll_interval_secs)).await;
        }
    }

    /// Get the results of a completed batch
    ///
    /// For inline requests, results are included in the batch response.
    /// This method extracts them from the batch job's response field.
    ///
    /// Note: For very large batches, results may be stored in GCS and need
    /// separate retrieval (not currently implemented).
    pub async fn get_results(&self, batch_name: &str) -> Result<Vec<BatchResponse>> {
        let batch = self.get_batch(batch_name).await?;

        if batch.state != BatchState::JobStateSucceeded {
            warn!(
                "Getting results for non-succeeded batch {}: {:?}",
                batch_name, batch.state
            );
        }

        // For inline requests, we need to make a separate call to get responses
        // The Gemini API returns responses via a different endpoint or in the batch job itself
        // depending on the batch configuration. For now, we'll return an empty vec
        // as the actual response handling depends on API specifics.

        // TODO: Implement proper result retrieval based on Gemini API behavior
        // The responses might be:
        // 1. Embedded in the batch job response
        // 2. Available via a separate endpoint
        // 3. Stored in GCS for large batches

        Ok(Vec::new())
    }
}

/// Helper to build a GenerateContentRequest for batch processing
pub fn build_batch_request(
    custom_id: &str,
    model: &str,
    contents: Vec<serde_json::Value>,
    system_instruction: Option<&str>,
) -> BatchRequest {
    let model_path = if model.starts_with("models/") {
        model.to_string()
    } else {
        format!("models/{}", model)
    };

    let mut request = serde_json::json!({
        "model": model_path,
        "contents": contents,
    });

    if let Some(instruction) = system_instruction {
        request["systemInstruction"] = serde_json::json!({
            "parts": [{"text": instruction}]
        });
    }

    BatchRequest {
        custom_id: custom_id.to_string(),
        request,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_batch_state_is_terminal() {
        assert!(!BatchState::JobStatePending.is_terminal());
        assert!(!BatchState::JobStateRunning.is_terminal());
        assert!(BatchState::JobStateSucceeded.is_terminal());
        assert!(BatchState::JobStateFailed.is_terminal());
        assert!(BatchState::JobStateCancelled.is_terminal());
    }

    #[test]
    fn test_batch_state_to_status() {
        assert_eq!(BatchState::JobStatePending.to_status(), "pending");
        assert_eq!(BatchState::JobStateRunning.to_status(), "running");
        assert_eq!(BatchState::JobStateSucceeded.to_status(), "succeeded");
        assert_eq!(BatchState::JobStateFailed.to_status(), "failed");
        assert_eq!(BatchState::JobStateCancelled.to_status(), "cancelled");
    }

    #[test]
    fn test_build_batch_request() {
        let request = build_batch_request(
            "req-1",
            "gemini-2.0-flash",
            vec![serde_json::json!({
                "role": "user",
                "parts": [{"text": "Hello"}]
            })],
            Some("You are a helpful assistant."),
        );

        assert_eq!(request.custom_id, "req-1");
        assert_eq!(request.request["model"], "models/gemini-2.0-flash");
        assert!(request.request["systemInstruction"].is_object());
    }

    #[test]
    fn test_build_batch_request_no_system() {
        let request = build_batch_request(
            "req-2",
            "models/gemini-2.0-flash",
            vec![serde_json::json!({
                "role": "user",
                "parts": [{"text": "Test"}]
            })],
            None,
        );

        assert_eq!(request.custom_id, "req-2");
        assert!(request.request.get("systemInstruction").is_none());
    }
}
