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
pub struct UploadFileResponse {
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
        let stores = self.stores.read().await;
        let store_info = stores.get(project_id)
            .ok_or_else(|| anyhow::anyhow!("No vector store for project"))?;

        // Upload file to OpenAI files endpoint (multipart)
        let file = tokio::fs::File::open(&file_path).await?;
        let file_name = file_path.file_name().and_then(|n| n.to_str()).unwrap_or("upload");
        let form = reqwest::multipart::Form::new()
            .file("file", &file_path)
            .context("Failed to create multipart file form")?
            .text("purpose", "assistants");

        let upload_res = self.client
            .request_multipart("files")
            .multipart(form)
            .send()
            .await
            .context("Failed to send file upload request")?
            .error_for_status()
            .context("Non-2xx from OpenAI on file upload")?
            .json::<UploadFileResponse>()
            .await
            .context("Failed to parse file upload response")?;

        // Attach file to the vector store
        let add_req = AddFileToVectorStoreRequest {
            file_id: upload_res.id.clone(),
        };
        self.client
            .request(Method::POST, &format!("vector_stores/{}/files", store_info.id))
            .json(&add_req)
            .send()
            .await
            .context("Failed to attach file to vector store")?
            .error_for_status()
            .context("Non-2xx from OpenAI on add file to vector store")?;

        // (Optional: update file_ids in memory)
        drop(stores); // release read lock before write
        let mut stores = self.stores.write().await;
        if let Some(store) = stores.get_mut(project_id) {
            store.file_ids.push(upload_res.id.clone());
        }

        Ok(upload_res.id)
    }

    // Optional: Add more management methods as needed.
}
