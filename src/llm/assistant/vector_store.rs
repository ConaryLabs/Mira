// src/llm/assistant/vector_store.rs

use crate::llm::client::OpenAIClient;
use reqwest::Method;
use serde::{Serialize, Deserialize};
use anyhow::{Result, Context};
use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::RwLock;
use chrono::{DateTime, Utc};
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
pub struct VectorStoreInfo {
    pub id: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub file_ids: Vec<String>,
    #[serde(default)]
    pub usage_bytes: i64,
}

#[derive(Serialize, Debug)]
pub struct CreateVectorStoreRequest {
    pub name: String,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Deserialize, Debug)]
pub struct VectorStoreResponse {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    // Add more fields if needed
}

#[derive(Deserialize, Debug)]
pub struct FileResponse {
    pub id: String,
    // Add more as needed
}

#[derive(Serialize, Debug)]
pub struct AddFileToVectorStoreRequest {
    pub file_id: String,
}

pub struct VectorStoreManager {
    client: Arc<OpenAIClient>,
    stores: Arc<RwLock<HashMap<String, VectorStoreInfo>>>, // project_id → VectorStoreInfo
}

impl VectorStoreManager {
    pub fn new(client: Arc<OpenAIClient>) -> Self {
        Self {
            client,
            stores: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new vector store for a project (returns vector store id).
    pub async fn create_project_store(&self, project_id: &str) -> Result<String> {
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
            name: res.name,
            created_at: Utc::now(),
            file_ids: vec![],
            usage_bytes: 0,
        };
        self.stores.write().await.insert(project_id.to_string(), info);

        Ok(res.id)
    }

    /// Upload a document and attach to the given project's vector store.
    pub async fn add_document(
        &self,
        project_id: &str,
        file_path: PathBuf,
    ) -> Result<String> {
        // First, get the vector store ID
        let stores = self.stores.read().await;
        let store_info = stores.get(project_id)
            .ok_or_else(|| anyhow::anyhow!("Vector store not found for project: {}", project_id))?;
        let store_id = store_info.id.clone();
        drop(stores);
        
        // Read file content
        let file_content = tokio::fs::read(&file_path).await
            .context("Failed to read file")?;
        
        let file_name = file_path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("document");
        
        // Create multipart form for file upload
        let form = reqwest::multipart::Form::new()
            .text("purpose", "assistants")
            .part("file", reqwest::multipart::Part::bytes(file_content)
                .file_name(file_name.to_string()));
        
        // Use the request_multipart method from OpenAIClient
        let file_res = self.client
            .request_multipart("files")
            .multipart(form)
            .send()
            .await
            .context("Failed to upload file")?
            .error_for_status()
            .context("Non-2xx from OpenAI when uploading file")?
            .json::<FileResponse>()
            .await
            .context("Failed to parse file upload response")?;
        
        // Add file to vector store
        let add_req = AddFileToVectorStoreRequest {
            file_id: file_res.id.clone(),
        };
        
        self.client
            .request(Method::POST, &format!("vector_stores/{}/files", store_id))
            .json(&add_req)
            .send()
            .await
            .context("Failed to add file to vector store")?
            .error_for_status()
            .context("Non-2xx from OpenAI when adding file to vector store")?;
        
        // Update our local tracking
        let mut stores = self.stores.write().await;
        if let Some(info) = stores.get_mut(project_id) {
            info.file_ids.push(file_res.id.clone());
        }
        
        Ok(file_res.id)
    }
    
    /// Get vector store info for a project
    pub async fn get_store_info(&self, project_id: &str) -> Option<VectorStoreInfo> {
        self.stores.read().await.get(project_id).cloned()
    }
    
    /// List all vector stores
    pub async fn list_stores(&self) -> Vec<(String, VectorStoreInfo)> {
        self.stores.read().await
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }
}
