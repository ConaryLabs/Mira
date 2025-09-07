// src/llm/responses/vector_store.rs
// Manages interactions with OpenAI's vector store capabilities for document retrieval.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::Utc;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::llm::client::OpenAIClient;

/// Manages OpenAI vector stores for document storage and retrieval.
#[derive(Clone)]
pub struct VectorStoreManager {
    client: Arc<OpenAIClient>,
    stores: Arc<RwLock<HashMap<String, VectorStoreInfo>>>,
}

/// Contains metadata about a cached vector store.
#[derive(Debug, Clone)]
pub struct VectorStoreInfo {
    pub id: String,
    pub name: String,
    pub created_at: i64,
    pub file_ids: Vec<String>,
    pub usage_bytes: i64,
}

impl VectorStoreManager {
    pub const PERSONAL_STORE_KEY: &'static str = "_personal";

    pub fn new(client: Arc<OpenAIClient>) -> Self {
        Self {
            client,
            stores: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Creates a new vector store for a given project identifier.
    pub async fn create_project_store(&self, project_id: &str) -> Result<String> {
        info!("Creating vector store for project: {}", project_id);
        
        let req = CreateVectorStoreRequest {
            name: format!("Project: {project_id}"),
            metadata: Some(serde_json::json!({
                "project_id": project_id,
                "created_at": Utc::now().to_rfc3339(),
                "type": "project_documents",
            })),
        };

        let res = self.client
            .request(Method::POST, "vector_stores")
            .json(&req)
            .send()
            .await
            .context("Failed to send create vector store request")?
            .error_for_status()
            .context("API error on vector store creation")?
            .json::<VectorStoreResponse>()
            .await
            .context("Failed to parse vector store response")?;

        let info = VectorStoreInfo {
            id: res.id.clone(),
            name: res.name.clone(),
            created_at: res.created_at,
            file_ids: vec![],
            usage_bytes: 0,
        };
        
        self.stores.write().await.insert(project_id.to_string(), info);
        info!("Successfully created vector store with ID: {}", res.id);
        
        Ok(res.id)
    }

    /// Retrieves the ID of the personal vector store, creating it if it doesn't exist.
    pub async fn get_or_create_personal_store(&self) -> Result<String> {
        if let Some(info) = self.get_store_info(Self::PERSONAL_STORE_KEY).await {
            Ok(info.id)
        } else {
            self.create_project_store(Self::PERSONAL_STORE_KEY).await
        }
    }

    /// Uploads a document and attaches it to the specified vector store.
    pub async fn add_document(&self, project_id_or_personal: &str, file_path: PathBuf) -> Result<String> {
        info!("Adding document to vector store: {:?}", file_path);
        
        let store_id = if let Some(store_info) = self.get_store_info(project_id_or_personal).await {
            store_info.id
        } else {
            self.create_project_store(project_id_or_personal).await?
        };

        let file_content = tokio::fs::read(&file_path).await.context("Failed to read file")?;
        
        let file_name = file_path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("document.txt");

        // The 'user_data' purpose is required for the new Responses API retrieval features.
        let form = reqwest::multipart::Form::new()
            .text("purpose", "user_data")
            .part("file", reqwest::multipart::Part::bytes(file_content)
                .file_name(file_name.to_string()));

        let file_response = self.client
            .request_multipart("files")
            .multipart(form)
            .send()
            .await
            .context("Failed to upload file")?
            .error_for_status()
            .context("API error on file upload")?
            .json::<FileResponse>()
            .await
            .context("Failed to parse file upload response")?;

        info!("File uploaded successfully with ID: {}", file_response.id);

        let attach_req = AttachFileRequest {
            file_id: file_response.id.clone(),
        };

        self.client
            .request(Method::POST, &format!("vector_stores/{store_id}/files"))
            .json(&attach_req)
            .send()
            .await
            .context("Failed to attach file to vector store")?
            .error_for_status()
            .context("API error on file attachment")?;

        if let Some(info) = self.stores.write().await.get_mut(project_id_or_personal) {
            info.file_ids.push(file_response.id.clone());
        }

        info!("Document attached successfully to vector store");
        Ok(file_response.id)
    }

    /// Searches for relevant content within a specified vector store.
    pub async fn search_documents(
        &self,
        project_id: Option<&str>,
        query: &str,
        max_results: usize,
    ) -> Result<Vec<SearchResult>> {
        let store_key = project_id.unwrap_or(Self::PERSONAL_STORE_KEY);
        
        let store_info = match self.get_store_info(store_key).await {
            Some(info) => info,
            None => {
                debug!("No vector store found for key: {}, returning empty search results.", store_key);
                return Ok(vec![]);
            }
        };

        info!("Searching vector store {} for query: '{}'", store_info.id, query);

        let search_req = SearchRequest {
            query: query.to_string(),
            max_results: max_results as u32,
        };

        let response = self.client
            .request(Method::POST, &format!("vector_stores/{}/search", store_info.id))
            .json(&search_req)
            .send()
            .await
            .context("Failed to send search request to vector store")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            
            if error_text.contains("indexing") || error_text.contains("processing") {
                warn!("Vector store is still indexing, returning empty results for now.");
            } else {
                warn!("Vector store search failed with status {}: {}", status, error_text);
            }
            return Ok(vec![]);
        }

        let search_response = response
            .json::<SearchResponse>()
            .await
            .context("Failed to parse search response")?;

        let results: Vec<SearchResult> = search_response.results
            .into_iter()
            .map(|r| SearchResult {
                content: r.content,
                score: r.score,
                file_id: r.file_id,
                metadata: r.metadata,
            })
            .collect();

        info!("Found {} search results.", results.len());
        Ok(results)
    }

    /// Retrieves cached information about a specific vector store.
    async fn get_store_info(&self, key: &str) -> Option<VectorStoreInfo> {
        self.stores.read().await.get(key).cloned()
    }

    /// Lists all cached vector stores.
    pub async fn list_stores(&self) -> Result<Vec<VectorStoreInfo>> {
        let stores = self.stores.read().await;
        Ok(stores.values().cloned().collect())
    }

    /// Deletes a vector store from OpenAI and removes it from the cache.
    pub async fn delete_store(&self, project_id: &str) -> Result<()> {
        if let Some(info) = self.get_store_info(project_id).await {
            self.client
                .request(Method::DELETE, &format!("vector_stores/{}", info.id))
                .send()
                .await
                .context("Failed to send delete vector store request")?
                .error_for_status()
                .context("API error on vector store deletion")?;
            
            self.stores.write().await.remove(project_id);
            info!("Deleted vector store for project: {}", project_id);
        }
        Ok(())
    }
}

// Internal request/response structs for serializing and deserializing API calls.
#[derive(Debug, Serialize)]
struct CreateVectorStoreRequest {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    metadata: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct VectorStoreResponse {
    id: String,
    name: String,
    created_at: i64,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)] // Allow unused fields as they are part of the API contract
struct FileResponse {
    id: String,
    filename: String,
    bytes: i64,
}

#[derive(Debug, Serialize)]
struct AttachFileRequest {
    file_id: String,
}

#[derive(Debug, Serialize)]
struct SearchRequest {
    query: String,
    max_results: u32,
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    results: Vec<SearchResultItem>,
}

#[derive(Debug, Deserialize)]
struct SearchResultItem {
    content: String,
    score: f32,
    file_id: Option<String>,
    metadata: Option<Value>,
}

/// Public representation of a search result from a vector store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub content: String,
    pub score: f32,
    pub file_id: Option<String>,
    pub metadata: Option<Value>,
}

impl SearchResult {
    /// Formats the search result for inclusion in a chat context prompt.
    pub fn format_for_context(&self) -> String {
        format!("[Retrieved content (score: {:.2})]: {}", self.score, self.content)
    }
}
