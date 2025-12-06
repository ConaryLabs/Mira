// src/api/ws/documents.rs

use anyhow::Result;
use base64::{Engine as _, engine::general_purpose};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::{
    api::{error::ApiError, ws::message::WsServerMessage},
    config::CONFIG,
    memory::features::document_processing::DocumentProcessor,
    state::AppState,
};

#[derive(Debug, Deserialize)]
pub struct DocumentCommand {
    pub method: String,
    pub params: Value,
}

#[derive(Debug, Deserialize)]
pub struct UploadParams {
    pub project_id: String,
    pub file_name: String,
    pub content: String,
    pub file_type: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SearchParams {
    pub project_id: String,
    pub query: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize {
    10
}

#[derive(Debug, Deserialize)]
pub struct RetrieveParams {
    pub document_id: String,
}

#[derive(Debug, Deserialize)]
pub struct ListParams {
    pub project_id: String,
}

#[derive(Debug, Deserialize)]
pub struct DeleteParams {
    pub document_id: String,
}

#[derive(Debug, Serialize)]
pub struct ProgressUpdate {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub file_name: String,
    pub progress: f32,
    pub status: String,
}

pub struct DocumentHandler {
    state: Arc<AppState>,
    processor: Arc<DocumentProcessor>,
}

impl DocumentHandler {
    pub fn new(state: Arc<AppState>) -> Self {
        let qdrant_url = CONFIG.qdrant_url.clone();

        let qdrant_client = qdrant_client::Qdrant::from_url(&qdrant_url)
            .build()
            .expect("Failed to create Qdrant client");

        let processor = Arc::new(DocumentProcessor::new(
            state.sqlite_pool.clone(),
            qdrant_client,
            state.openai_embedding_client.clone(),
        ));

        Self { state, processor }
    }

    pub async fn handle_command(
        &self,
        command: DocumentCommand,
        progress_tx: Option<mpsc::UnboundedSender<WsServerMessage>>,
    ) -> Result<WsServerMessage> {
        match command.method.as_str() {
            "documents.upload" => {
                let params: UploadParams = serde_json::from_value(command.params)?;
                self.handle_upload(params, progress_tx).await
            }
            "documents.search" => {
                let params: SearchParams = serde_json::from_value(command.params)?;
                self.handle_search(params).await
            }
            "documents.retrieve" => {
                let params: RetrieveParams = serde_json::from_value(command.params)?;
                self.handle_retrieve(params).await
            }
            "documents.list" => {
                let params: ListParams = serde_json::from_value(command.params)?;
                self.handle_list(params).await
            }
            "documents.delete" => {
                let params: DeleteParams = serde_json::from_value(command.params)?;
                self.handle_delete(params).await
            }
            _ => Err(
                ApiError::bad_request(format!("Unknown document method: {}", command.method))
                    .into(),
            ),
        }
    }

    async fn handle_upload(
        &self,
        params: UploadParams,
        progress_tx: Option<mpsc::UnboundedSender<WsServerMessage>>,
    ) -> Result<WsServerMessage> {
        let file_content = general_purpose::STANDARD
            .decode(&params.content)
            .map_err(|e| ApiError::bad_request(format!("Invalid base64 content: {}", e)))?;

        let temp_dir = std::env::temp_dir();
        let temp_path = temp_dir.join(&params.file_name);
        tokio::fs::write(&temp_path, file_content).await?;

        if let Some(ref tx) = progress_tx {
            let _ = tx.send(WsServerMessage::Data {
                data: json!({
                    "type": "document_processing_started",
                    "file_name": params.file_name.clone(),
                    "progress": 0.0,
                    "status": "starting"
                }),
                request_id: None,
            });
        }

        let progress_callback: Option<Box<dyn Fn(f32) + Send + Sync>> = progress_tx.map(|tx| {
            let file_name = params.file_name.clone();
            Box::new(move |progress: f32| {
                let _ = tx.send(WsServerMessage::Data {
                    data: json!({
                        "type": "document_processing_progress",
                        "file_name": file_name.clone(),
                        "progress": progress,
                        "status": "processing"
                    }),
                    request_id: None,
                });
            }) as Box<dyn Fn(f32) + Send + Sync>
        });

        match self
            .processor
            .process_document(&temp_path, &params.project_id, progress_callback)
            .await
        {
            Ok(processed) => {
                let _ = tokio::fs::remove_file(&temp_path).await;

                Ok(WsServerMessage::Data {
                    data: json!({
                        "type": "document_processed",
                        "document": {
                            "id": processed.id,
                            "file_name": processed.file_name,
                            "file_type": processed.file_type,
                            "size_bytes": processed.size_bytes,
                            "word_count": processed.word_count,
                            "chunk_count": processed.chunks.len(),
                            "metadata": processed.metadata,
                        }
                    }),
                    request_id: None,
                })
            }
            Err(e) => {
                let _ = tokio::fs::remove_file(&temp_path).await;

                if e.to_string().contains("already exists") {
                    Err(ApiError::conflict(e.to_string()).into())
                } else {
                    Err(ApiError::internal(format!("Failed to process document: {}", e)).into())
                }
            }
        }
    }

    async fn handle_search(&self, params: SearchParams) -> Result<WsServerMessage> {
        let results = self
            .processor
            .search_documents(&params.project_id, &params.query, params.limit)
            .await?;

        Ok(WsServerMessage::Data {
            data: json!({
                "type": "search_results",
                "results": results,
                "query": params.query,
                "total": results.len()
            }),
            request_id: None,
        })
    }

    async fn handle_retrieve(&self, params: RetrieveParams) -> Result<WsServerMessage> {
        if let Some(document) = self
            .processor
            .retrieve_document(&params.document_id)
            .await?
        {
            let file_content = tokio::fs::read(&document.file_path).await?;
            let encoded = general_purpose::STANDARD.encode(&file_content);

            Ok(WsServerMessage::Data {
                data: json!({
                    "type": "document_content",
                    "document_id": document.id,
                    "file_name": document.file_name,
                    "file_type": document.file_type,
                    "content": encoded,
                    "size_bytes": document.size_bytes
                }),
                request_id: None,
            })
        } else {
            Err(ApiError::not_found(format!("Document not found: {}", params.document_id)).into())
        }
    }

    async fn handle_list(&self, params: ListParams) -> Result<WsServerMessage> {
        let qdrant_client = qdrant_client::Qdrant::from_url(&CONFIG.qdrant_url)
            .build()
            .expect("Failed to create Qdrant client");

        let storage = crate::memory::features::document_processing::DocumentStorage::new(
            self.state.sqlite_pool.clone(),
            qdrant_client,
            self.state.openai_embedding_client.clone(),
        );

        let documents = storage.list_documents(&params.project_id).await?;

        Ok(WsServerMessage::Data {
            data: json!({
                "type": "document_list",
                "documents": documents.into_iter().map(|doc| {
                    json!({
                        "id": doc.id,
                        "file_name": doc.file_name,
                        "file_type": doc.file_type,
                        "size_bytes": doc.size_bytes,
                        "created_at": doc.created_at,
                    })
                }).collect::<Vec<_>>(),
                "project_id": params.project_id
            }),
            request_id: None,
        })
    }

    async fn handle_delete(&self, params: DeleteParams) -> Result<WsServerMessage> {
        self.processor.delete_document(&params.document_id).await?;

        Ok(WsServerMessage::Data {
            data: json!({
                "type": "document_deleted",
                "document_id": params.document_id,
                "status": "success"
            }),
            request_id: None,
        })
    }
}
