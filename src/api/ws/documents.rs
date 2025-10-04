// src/api/ws/documents.rs
//! WebSocket handler for document operations

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::mpsc;
use base64::{Engine as _, engine::general_purpose};

use crate::{
    api::{error::ApiError, ws::message::WsServerMessage},
    memory::features::document_processing::DocumentProcessor,
    state::AppState,
};

/// Document command request structure
#[derive(Debug, Deserialize)]
pub struct DocumentCommand {
    pub method: String,
    pub params: Value,
}

/// Document upload parameters
#[derive(Debug, Deserialize)]
pub struct UploadParams {
    pub project_id: String,
    pub file_name: String,
    pub content: String,  // Base64 encoded file content
    pub file_type: Option<String>,  // Optional - will be derived from file_name if not provided
}

/// Document search parameters
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

/// Document retrieve parameters
#[derive(Debug, Deserialize)]
pub struct RetrieveParams {
    pub document_id: String,
}

/// Document list parameters
#[derive(Debug, Deserialize)]
pub struct ListParams {
    pub project_id: String,
}

/// Document delete parameters
#[derive(Debug, Deserialize)]
pub struct DeleteParams {
    pub document_id: String,
}

/// Progress update message
#[derive(Debug, Serialize)]
pub struct ProgressUpdate {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub file_name: String,
    pub progress: f32,
    pub status: String,
}

/// Detect file type from filename extension
fn detect_file_type(file_name: &str) -> String {
    let extension = file_name
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_lowercase();
    
    match extension.as_str() {
        "pdf" => "pdf",
        "docx" | "doc" => "docx",
        "txt" => "txt",
        "md" | "markdown" => "markdown",
        _ => "unknown",
    }.to_string()
}

/// Document handler for WebSocket commands
pub struct DocumentHandler {
    state: Arc<AppState>,
    processor: Arc<DocumentProcessor>,
}

impl DocumentHandler {
    /// Create a new document handler
    pub fn new(state: Arc<AppState>) -> Self {
        // Create Qdrant client using the same URL from config
        let qdrant_url = std::env::var("MIRA_QDRANT_URL")
            .unwrap_or_else(|_| "http://localhost:6334".to_string());
        
        let qdrant_client = qdrant_client::Qdrant::from_url(&qdrant_url)
            .build()
            .expect("Failed to create Qdrant client");
        
        let processor = Arc::new(DocumentProcessor::new(
            state.sqlite_pool.clone(),
            qdrant_client,
        ));
        
        Self {
            state,
            processor,
        }
    }
    
    /// Handle document command
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
            _ => {
                Err(ApiError::bad_request(format!("Unknown document method: {}", command.method)).into())
            }
        }
    }
    
    /// Handle document upload
    async fn handle_upload(
        &self,
        params: UploadParams,
        progress_tx: Option<mpsc::UnboundedSender<WsServerMessage>>,
    ) -> Result<WsServerMessage> {
        // Decode base64 content
        let file_content = general_purpose::STANDARD.decode(&params.content)
            .map_err(|e| ApiError::bad_request(format!("Invalid base64 content: {}", e)))?;
        
        // Derive file_type from filename if not provided
        let file_type = params.file_type
            .clone()
            .unwrap_or_else(|| detect_file_type(&params.file_name));
        
        // Create temporary file
        let temp_dir = std::env::temp_dir();
        let temp_path = temp_dir.join(&params.file_name);
        tokio::fs::write(&temp_path, file_content).await?;
        
        // Send initial progress
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
        
        // Create progress callback
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
        
        // Process document
        match self.processor.process_document(
            &temp_path,
            &params.project_id,
            progress_callback
        ).await {
            Ok(processed) => {
                // Clean up temp file
                let _ = tokio::fs::remove_file(&temp_path).await;
                
                // Return success with document info
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
                // Clean up temp file
                let _ = tokio::fs::remove_file(&temp_path).await;
                
                // Check for duplicate error
                if e.to_string().contains("already exists") {
                    Err(ApiError::conflict(e.to_string()).into())
                } else {
                    Err(ApiError::internal(format!("Failed to process document: {}", e)).into())
                }
            }
        }
    }
    
    /// Handle document search
    async fn handle_search(&self, params: SearchParams) -> Result<WsServerMessage> {
        let results = self.processor
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
    
    /// Handle document retrieval
    async fn handle_retrieve(&self, params: RetrieveParams) -> Result<WsServerMessage> {
        if let Some(document) = self.processor.retrieve_document(&params.document_id).await? {
            // Read file content for download
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
    
    /// Handle document list
    async fn handle_list(&self, params: ListParams) -> Result<WsServerMessage> {
        // Create Qdrant client using the same URL from config
        let qdrant_url = std::env::var("MIRA_QDRANT_URL")
            .unwrap_or_else(|_| "http://localhost:6334".to_string());
        
        let qdrant_client = qdrant_client::Qdrant::from_url(&qdrant_url)
            .build()
            .expect("Failed to create Qdrant client");
        
        let storage = crate::memory::features::document_processing::DocumentStorage::new(
            self.state.sqlite_pool.clone(),
            qdrant_client,
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
    
    /// Handle document deletion
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
