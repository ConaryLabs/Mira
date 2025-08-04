// src/llm/assistant/vector_store.rs

use crate::llm::client::OpenAIClient;
use reqwest::Method;
use serde::{Serialize, Deserialize, Deserializer};
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

// Custom deserializer for timestamps that can handle both Unix timestamps and RFC3339 strings
fn deserialize_timestamp<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum TimestampFormat {
        UnixTimestamp(i64),
        DateTimeString(String),
    }
    
    match TimestampFormat::deserialize(deserializer)? {
        TimestampFormat::UnixTimestamp(ts) => {
            DateTime::from_timestamp(ts, 0)
                .ok_or_else(|| serde::de::Error::custom("Invalid Unix timestamp"))
        }
        TimestampFormat::DateTimeString(s) => {
            DateTime::parse_from_rfc3339(&s)
                .map(|dt| dt.with_timezone(&Utc))
                .map_err(serde::de::Error::custom)
        }
    }
}

#[derive(Deserialize, Debug)]
pub struct VectorStoreResponse {
    pub id: String,
    pub name: String,
    #[serde(deserialize_with = "deserialize_timestamp")]
    pub created_at: DateTime<Utc>,
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
    stores: Arc<RwLock<HashMap<String, VectorStoreInfo>>>, // key → VectorStoreInfo (project_id or PERSONAL_STORE_KEY)
}

impl VectorStoreManager {
    pub const PERSONAL_STORE_KEY: &'static str = "__personal";

    pub fn new(client: Arc<OpenAIClient>) -> Self {
        Self {
            client,
            stores: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new vector store for a project or for personal docs (returns vector store id).
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
            name: res.name.clone(),
            created_at: res.created_at,
            file_ids: vec![],
            usage_bytes: 0,
        };
        self.stores.write().await.insert(project_id.to_string(), info);

        Ok(res.id)
    }

    /// Get or create the default personal vector store (for non-project docs).
    pub async fn get_or_create_personal_store(&self) -> Result<String> {
        if let Some(info) = self.get_store_info(Self::PERSONAL_STORE_KEY).await {
            Ok(info.id)
        } else {
            self.create_project_store(Self::PERSONAL_STORE_KEY).await
        }
    }

    /// Upload a document and attach to the given vector store (project or personal).
    pub async fn add_document(&self, project_id_or_personal: &str, file_path: PathBuf) -> Result<String> {
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
            .text("purpose", "assistants")
            .part("file", reqwest::multipart::Part::bytes(file_content)
                .file_name(file_name.to_string()));

        let file_response = self.client
            .request(Method::POST, "files")
            .multipart(form)
            .send()
            .await
            .context("Failed to upload file")?
            .error_for_status()
            .context("Non-2xx from OpenAI file upload")?
            .json::<FileResponse>()
            .await
            .context("Failed to parse file upload response")?;

        // Attach file to vector store
        let attach_req = AddFileToVectorStoreRequest {
            file_id: file_response.id.clone(),
        };

        self.client
            .request(Method::POST, &format!("vector_stores/{}/files", store_id))
            .json(&attach_req)
            .send()
            .await
            .context("Failed to attach file to vector store")?
            .error_for_status()
            .context("Non-2xx from attach file")?;

        // Update local store info
        let mut stores = self.stores.write().await;
        if let Some(store_info) = stores.get_mut(project_id_or_personal) {
            store_info.file_ids.push(file_response.id.clone());
        }

        Ok(file_response.id)
    }

    /// Get vector store info for a project or personal store
    pub async fn get_store_info(&self, key: &str) -> Option<VectorStoreInfo> {
        self.stores.read().await.get(key).cloned()
    }
}
