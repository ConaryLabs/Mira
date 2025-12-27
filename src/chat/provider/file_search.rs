//! Gemini File Search API client for RAG functionality
//!
//! Provides per-project semantic document search via Gemini's FileSearch stores.
//! Documents are chunked, embedded, and searchable using the file_search tool.

use anyhow::{bail, Result};
use reqwest::Client as HttpClient;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;
use tokio::fs::File;
use tokio::io::AsyncReadExt;

const API_BASE: &str = "https://generativelanguage.googleapis.com/v1beta";
const UPLOAD_BASE: &str = "https://generativelanguage.googleapis.com/upload/v1beta";
const DEFAULT_TIMEOUT_SECS: u64 = 60;

/// Client for Gemini File Search API
pub struct FileSearchClient {
    client: HttpClient,
    api_key: String,
}

impl FileSearchClient {
    /// Create a new FileSearchClient
    pub fn new(api_key: String) -> Self {
        Self {
            client: HttpClient::new(),
            api_key,
        }
    }

    /// Create from environment variable
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("GEMINI_API_KEY")
            .map_err(|_| anyhow::anyhow!("GEMINI_API_KEY not set"))?;
        Ok(Self::new(api_key))
    }

    // ========================================================================
    // Store Management
    // ========================================================================

    /// Create a new FileSearch store
    ///
    /// Returns the store name (e.g., "fileSearchStores/abc123")
    pub async fn create_store(&self, display_name: &str) -> Result<FileSearchStore> {
        let url = format!("{}{}?key={}", API_BASE, "/fileSearchStores", self.api_key);

        let request = CreateStoreRequest {
            display_name: display_name.to_string(),
        };

        let response = self.client
            .post(&url)
            .json(&request)
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("Failed to create store: {} - {}", status, body);
        }

        let store: FileSearchStore = response.json().await?;
        tracing::info!("Created FileSearch store: {}", store.name);
        Ok(store)
    }

    /// Get a FileSearch store by name
    pub async fn get_store(&self, store_name: &str) -> Result<FileSearchStore> {
        let url = format!("{}/{}?key={}", API_BASE, store_name, self.api_key);

        let response = self.client
            .get(&url)
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("Failed to get store: {} - {}", status, body);
        }

        Ok(response.json().await?)
    }

    /// List all FileSearch stores
    pub async fn list_stores(&self, page_size: Option<u32>) -> Result<Vec<FileSearchStore>> {
        let page_size = page_size.unwrap_or(20).min(20);
        let url = format!(
            "{}/fileSearchStores?pageSize={}&key={}",
            API_BASE, page_size, self.api_key
        );

        let response = self.client
            .get(&url)
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("Failed to list stores: {} - {}", status, body);
        }

        let list: ListStoresResponse = response.json().await?;
        Ok(list.file_search_stores.unwrap_or_default())
    }

    /// Delete a FileSearch store
    ///
    /// Set `force` to true to delete even if documents exist
    pub async fn delete_store(&self, store_name: &str, force: bool) -> Result<()> {
        let url = format!(
            "{}/{}?force={}&key={}",
            API_BASE, store_name, force, self.api_key
        );

        let response = self.client
            .delete(&url)
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("Failed to delete store: {} - {}", status, body);
        }

        tracing::info!("Deleted FileSearch store: {}", store_name);
        Ok(())
    }

    // ========================================================================
    // Document Management
    // ========================================================================

    /// Upload a file to a FileSearch store
    ///
    /// Returns an operation that can be polled for completion
    pub async fn upload_file(
        &self,
        store_name: &str,
        file_path: &Path,
        display_name: Option<&str>,
        metadata: Option<Vec<CustomMetadata>>,
    ) -> Result<Operation> {
        // Read file content
        let mut file = File::open(file_path).await?;
        let mut content = Vec::new();
        file.read_to_end(&mut content).await?;

        // Detect MIME type
        let mime_type = mime_guess::from_path(file_path)
            .first_or_octet_stream()
            .to_string();

        // Build multipart request
        let file_name = file_path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file");

        let display = display_name.unwrap_or(file_name);

        let url = format!(
            "{}/{}:uploadToFileSearchStore?key={}",
            UPLOAD_BASE, store_name, self.api_key
        );

        // Create metadata part
        let metadata_json = serde_json::json!({
            "displayName": display,
            "mimeType": mime_type,
            "customMetadata": metadata.unwrap_or_default(),
        });

        let form = reqwest::multipart::Form::new()
            .text("metadata", metadata_json.to_string())
            .part("file", reqwest::multipart::Part::bytes(content)
                .file_name(file_name.to_string())
                .mime_str(&mime_type)?);

        let response = self.client
            .post(&url)
            .multipart(form)
            .timeout(Duration::from_secs(120)) // Longer timeout for uploads
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("Failed to upload file: {} - {}", status, body);
        }

        let operation: Operation = response.json().await?;
        tracing::info!("Started upload operation: {}", operation.name);
        Ok(operation)
    }

    /// Import an already-uploaded file into a FileSearch store
    pub async fn import_file(
        &self,
        store_name: &str,
        file_name: &str,
        metadata: Option<Vec<CustomMetadata>>,
    ) -> Result<Operation> {
        let url = format!(
            "{}/{}:importFile?key={}",
            API_BASE, store_name, self.api_key
        );

        let request = ImportFileRequest {
            file_name: file_name.to_string(),
            custom_metadata: metadata,
            chunking_config: None,
        };

        let response = self.client
            .post(&url)
            .json(&request)
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("Failed to import file: {} - {}", status, body);
        }

        let operation: Operation = response.json().await?;
        Ok(operation)
    }

    /// Check operation status
    pub async fn get_operation(&self, operation_name: &str) -> Result<Operation> {
        let url = format!("{}/{}?key={}", API_BASE, operation_name, self.api_key);

        let response = self.client
            .get(&url)
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("Failed to get operation: {} - {}", status, body);
        }

        Ok(response.json().await?)
    }

    /// Wait for an operation to complete
    pub async fn wait_for_operation(
        &self,
        operation_name: &str,
        max_wait_secs: u64,
    ) -> Result<Operation> {
        let start = std::time::Instant::now();
        let max_wait = Duration::from_secs(max_wait_secs);

        loop {
            let op = self.get_operation(operation_name).await?;

            if op.done.unwrap_or(false) {
                if let Some(error) = &op.error {
                    bail!("Operation failed: {} - {}", error.code, error.message);
                }
                return Ok(op);
            }

            if start.elapsed() > max_wait {
                bail!("Operation timed out after {} seconds", max_wait_secs);
            }

            // Poll every 2 seconds
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }
}

// ============================================================================
// Request/Response Types
// ============================================================================

#[derive(Serialize)]
struct CreateStoreRequest {
    #[serde(rename = "displayName")]
    display_name: String,
}

#[derive(Deserialize)]
struct ListStoresResponse {
    #[serde(rename = "fileSearchStores")]
    file_search_stores: Option<Vec<FileSearchStore>>,
    #[serde(rename = "nextPageToken")]
    next_page_token: Option<String>,
}

/// A FileSearch store resource
#[derive(Debug, Clone, Deserialize)]
pub struct FileSearchStore {
    /// Resource name (e.g., "fileSearchStores/abc123")
    pub name: String,
    /// Human-readable display name
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
    /// Creation timestamp (RFC 3339)
    #[serde(rename = "createTime")]
    pub create_time: Option<String>,
    /// Last update timestamp (RFC 3339)
    #[serde(rename = "updateTime")]
    pub update_time: Option<String>,
    /// Number of active (ready) documents
    #[serde(rename = "activeDocumentsCount", default)]
    pub active_documents_count: u32,
    /// Number of pending (processing) documents
    #[serde(rename = "pendingDocumentsCount", default)]
    pub pending_documents_count: u32,
    /// Number of failed documents
    #[serde(rename = "failedDocumentsCount", default)]
    pub failed_documents_count: u32,
    /// Total size in bytes
    #[serde(rename = "sizeBytes", default)]
    pub size_bytes: u64,
}

/// Custom metadata for document filtering
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomMetadata {
    pub key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub string_value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub numeric_value: Option<f64>,
}

#[derive(Serialize)]
struct ImportFileRequest {
    #[serde(rename = "fileName")]
    file_name: String,
    #[serde(rename = "customMetadata", skip_serializing_if = "Option::is_none")]
    custom_metadata: Option<Vec<CustomMetadata>>,
    #[serde(rename = "chunkingConfig", skip_serializing_if = "Option::is_none")]
    chunking_config: Option<ChunkingConfig>,
}

#[derive(Serialize)]
struct ChunkingConfig {
    #[serde(rename = "whiteSpaceConfig", skip_serializing_if = "Option::is_none")]
    white_space_config: Option<WhiteSpaceConfig>,
}

#[derive(Serialize)]
struct WhiteSpaceConfig {
    #[serde(rename = "maxTokensPerChunk")]
    max_tokens_per_chunk: u32,
    #[serde(rename = "maxOverlapTokens")]
    max_overlap_tokens: u32,
}

/// Long-running operation
#[derive(Debug, Clone, Deserialize)]
pub struct Operation {
    /// Operation name
    pub name: String,
    /// Whether the operation is complete
    pub done: Option<bool>,
    /// Error if operation failed
    pub error: Option<OperationError>,
    /// Result metadata
    pub metadata: Option<serde_json::Value>,
    /// Operation response
    pub response: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OperationError {
    pub code: i32,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_store_request() {
        let req = CreateStoreRequest {
            display_name: "test-store".into(),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("displayName"));
        assert!(json.contains("test-store"));
    }

    #[test]
    fn test_custom_metadata() {
        let meta = vec![
            CustomMetadata {
                key: "author".into(),
                string_value: Some("John".into()),
                numeric_value: None,
            },
            CustomMetadata {
                key: "year".into(),
                string_value: None,
                numeric_value: Some(2024.0),
            },
        ];
        let json = serde_json::to_string(&meta).unwrap();
        assert!(json.contains("author"));
        assert!(json.contains("John"));
        assert!(json.contains("2024"));
    }
}
