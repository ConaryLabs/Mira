// src/llm/responses/vector_store.rs
// Phase 6: OpenAI Vector Store integration for document retrieval

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::llm::client::OpenAIClient;

/// Manages OpenAI vector stores for document storage and retrieval
#[derive(Clone)]
pub struct VectorStoreManager {
    client: Arc<OpenAIClient>,
    stores: Arc<RwLock<HashMap<String, VectorStoreInfo>>>,
}

#[derive(Debug, Clone)]
struct VectorStoreInfo {
    id: String,
    name: String,
    created_at: i64,
    file_ids: Vec<String>,
    usage_bytes: i64,
}

impl VectorStoreManager {
    pub const PERSONAL_STORE_KEY: &'static str = "_personal";

    pub fn new(client: Arc<OpenAIClient>) -> Self {
        Self {
            client,
            stores: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new vector store for a project
    pub async fn create_project_store(&self, project_id: &str) -> Result<String> {
        info!("📦 Creating vector store for project: {}", project_id);
        
        let req = CreateVectorStoreRequest {
            name: format!("Project: {}", project_id),
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
            .context("Non-2xx from OpenAI vector store create")?
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
        info!("✅ Created vector store: {}", res.id);
        
        Ok(res.id)
    }

    /// Get or create the default personal vector store
    pub async fn get_or_create_personal_store(&self) -> Result<String> {
        if let Some(info) = self.get_store_info(Self::PERSONAL_STORE_KEY).await {
            Ok(info.id)
        } else {
            self.create_project_store(Self::PERSONAL_STORE_KEY).await
        }
    }

    /// Upload a document and attach to the given vector store
    pub async fn add_document(&self, project_id_or_personal: &str, file_path: PathBuf) -> Result<String> {
        info!("📄 Adding document to vector store: {:?}", file_path);
        
        // Ensure vector store exists
        let store_id = if let Some(store_info) = self.get_store_info(project_id_or_personal).await {
            store_info.id
        } else {
            self.create_project_store(project_id_or_personal).await?
        };

        // Upload file to OpenAI
        let file_content = tokio::fs::read(&file_path).await
            .context("Failed to read file")?;
        
        let file_name = file_path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("document.txt");

        // Create multipart form for file upload
        let form = reqwest::multipart::Form::new()
            .text("purpose", "assistants")  // Use "assistants" for vector store files
            .part("file", reqwest::multipart::Part::bytes(file_content)
                .file_name(file_name.to_string()));

        let file_response = self.client
            .request_multipart("files")
            .multipart(form)
            .send()
            .await
            .context("Failed to upload file")?
            .error_for_status()
            .context("Non-2xx from OpenAI file upload")?
            .json::<FileResponse>()
            .await
            .context("Failed to parse file upload response")?;

        info!("📤 File uploaded: {}", file_response.id);

        // Attach file to vector store
        let attach_req = AttachFileRequest {
            file_id: file_response.id.clone(),
        };

        self.client
            .request(Method::POST, &format!("vector_stores/{}/files", store_id))
            .json(&attach_req)
            .send()
            .await
            .context("Failed to attach file to vector store")?
            .error_for_status()
            .context("Non-2xx from file attachment")?;

        // Update local cache
        if let Some(mut info) = self.stores.write().await.get_mut(project_id_or_personal) {
            info.file_ids.push(file_response.id.clone());
        }

        info!("✅ Document attached to vector store");
        Ok(file_response.id)
    }

    /// Search for relevant content in a vector store
    pub async fn search_documents(
        &self,
        project_id: Option<&str>,
        query: &str,
        max_results: usize,
    ) -> Result<Vec<SearchResult>> {
        // Determine which store to search
        let store_key = project_id.unwrap_or(Self::PERSONAL_STORE_KEY);
        
        let store_info = match self.get_store_info(store_key).await {
            Some(info) => info,
            None => {
                debug!("No vector store found for key: {}", store_key);
                return Ok(vec![]);
            }
        };

        info!("🔍 Searching vector store {} for: {}", store_info.id, query);

        // Search the vector store
        let search_req = SearchRequest {
            query: query.to_string(),
            max_results: max_results as u32,
        };

        let response = self.client
            .request(Method::POST, &format!("vector_stores/{}/search", store_info.id))
            .json(&search_req)
            .send()
            .await
            .context("Failed to search vector store")?;

        // Check if the search API is available
        if response.status() == 404 {
            warn!("Vector store search API not available, falling back to retrieval through assistant");
            return self.search_via_assistant(&store_info.id, query, max_results).await;
        }

        let search_response = response
            .error_for_status()
            .context("Non-2xx from vector store search")?
            .json::<SearchResponse>()
            .await
            .context("Failed to parse search response")?;

        let results = search_response.results
            .into_iter()
            .map(|r| SearchResult {
                content: r.content,
                score: r.score,
                file_id: r.file_id,
                metadata: r.metadata,
            })
            .collect();

        info!("✅ Found {} search results", results.len());
        Ok(results)
    }

    /// Alternative search using assistant retrieval (fallback)
    async fn search_via_assistant(
        &self,
        store_id: &str,
        query: &str,
        max_results: usize,
    ) -> Result<Vec<SearchResult>> {
        debug!("Using assistant retrieval fallback for vector store search");
        
        // Create a temporary assistant with retrieval enabled
        let assistant_req = serde_json::json!({
            "model": "gpt-5",
            "instructions": "You are a document search assistant. Return relevant excerpts.",
            "tools": [{
                "type": "retrieval"
            }],
            "file_ids": [],  // Will be populated from vector store
            "metadata": {
                "vector_store_id": store_id
            }
        });

        // This is a simplified version - in production, you'd want to:
        // 1. Create an assistant
        // 2. Create a thread
        // 3. Add the query as a message
        // 4. Run the assistant
        // 5. Parse the retrieval results
        
        // For now, return empty results as this is a fallback
        warn!("Assistant retrieval not fully implemented, returning empty results");
        Ok(vec![])
    }

    /// Get store information from cache
    async fn get_store_info(&self, key: &str) -> Option<VectorStoreInfo> {
        self.stores.read().await.get(key).cloned()
    }

    /// List all vector stores
    pub async fn list_stores(&self) -> Result<Vec<VectorStoreInfo>> {
        let stores = self.stores.read().await;
        Ok(stores.values().cloned().collect())
    }

    /// Delete a vector store
    pub async fn delete_store(&self, project_id: &str) -> Result<()> {
        if let Some(info) = self.get_store_info(project_id).await {
            self.client
                .request(Method::DELETE, &format!("vector_stores/{}", info.id))
                .send()
                .await
                .context("Failed to delete vector store")?
                .error_for_status()
                .context("Non-2xx from vector store deletion")?;
            
            self.stores.write().await.remove(project_id);
            info!("🗑️ Deleted vector store for project: {}", project_id);
        }
        Ok(())
    }
}

// Request/Response types

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

/// Public search result type
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub content: String,
    pub score: f32,
    pub file_id: Option<String>,
    pub metadata: Option<Value>,
}

impl SearchResult {
    /// Format for inclusion in chat context
    pub fn format_for_context(&self) -> String {
        format!("[Retrieved content (score: {:.2})]: {}", self.score, self.content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_result_formatting() {
        let result = SearchResult {
            content: "Test content".to_string(),
            score: 0.95,
            file_id: Some("file-123".to_string()),
            metadata: None,
        };
        
        let formatted = result.format_for_context();
        assert!(formatted.contains("Retrieved content"));
        assert!(formatted.contains("0.95"));
        assert!(formatted.contains("Test content"));
    }
}
