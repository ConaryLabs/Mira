// src/services/document.rs

use crate::services::{MemoryService, ChatService};
use crate::llm::schema::MiraStructuredReply;
use crate::llm::assistant::VectorStoreManager;
use std::sync::Arc;
use std::path::{Path, PathBuf};
use anyhow::Result;
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Copy)]
pub enum DocumentDestination {
    PersonalMemory,      // Goes to Qdrant
    ProjectVectorStore,  // Goes to OpenAI vector store (must have project_id)
    Both,                // Hybrid storage (must have project_id)
}

pub struct DocumentService {
    memory_service: Arc<MemoryService>,
    chat_service: Arc<ChatService>,
    vector_store_manager: Arc<VectorStoreManager>,
}

impl DocumentService {
    pub fn new(
        memory_service: Arc<MemoryService>,
        chat_service: Arc<ChatService>,
        vector_store_manager: Arc<VectorStoreManager>,
    ) -> Self {
        Self {
            memory_service,
            chat_service,
            vector_store_manager,
        }
    }

    pub async fn process_document(
        &self,
        file_path: &Path,
        content: &str,
        project_id: Option<&str>,
    ) -> Result<()> {
        let destination = self.analyze_and_route(file_path, content, project_id).await?;

        match destination {
            DocumentDestination::PersonalMemory => {
                self.process_for_personal_memory(content).await?;
            }
            DocumentDestination::ProjectVectorStore => {
                let store_key = project_id.expect("Project ID required for project vector store upload");
                self.vector_store_manager
                    .add_document(store_key, file_path.to_path_buf())
                    .await?;

                self.process_for_project_vector_store(content, project_id).await?;
            }
            DocumentDestination::Both => {
                let store_key = project_id.expect("Project ID required for vector store upload (Both)");
                self.vector_store_manager
                    .add_document(store_key, file_path.to_path_buf())
                    .await?;
                self.process_for_project_vector_store(content, project_id).await?;
                self.process_for_personal_memory(content).await?;
            }
        }

        Ok(())
    }

    async fn analyze_and_route(
        &self,
        file_path: &Path,
        content: &str,
        project_id: Option<&str>,
    ) -> Result<DocumentDestination> {
        let extension = file_path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        
        let file_name = file_path.file_name()
            .and_then(|f| f.to_str())
            .unwrap_or("document");

        // 1. STRICTLY personal: filename or content markers (check *first*)
        if file_name.to_lowercase().contains("diary")
            || file_name.to_lowercase().contains("personal")
            || file_name.to_lowercase().contains("journal")
            || content.to_lowercase().contains("dear diary")
            || content.to_lowercase().contains("personal journal")
        {
            return Ok(DocumentDestination::PersonalMemory);
        }

        // 2. Technical file extensions REQUIRE project ID
        let technical_exts = ["md", "pdf", "txt", "rs", "js", "py"];
        if technical_exts.contains(&extension) {
            if project_id.is_none() {
                return Err(anyhow::anyhow!(
                    "Technical documents require a project ID for vector storage"
                ));
            }
            return Ok(DocumentDestination::ProjectVectorStore);
        }

        // 3. Large "both" content (still must require project ID for upload)
        if content.len() > 5000 && (content.contains("insight") || content.contains("reflection")) {
            if project_id.is_none() {
                return Err(anyhow::anyhow!(
                    "Both destination requires a project ID for vector storage"
                ));
            }
            return Ok(DocumentDestination::Both);
        }

        // 4. By default, route to project vector store (if and only if project ID is set)
        if project_id.is_some() {
            Ok(DocumentDestination::ProjectVectorStore)
        } else {
            Err(anyhow::anyhow!(
                "Project ID required for project vector store upload"
            ))
        }
    }

    async fn process_for_personal_memory(&self, content: &str) -> Result<()> {
        let doc_response = MiraStructuredReply {
            output: content.to_string(),
            persona: "system".to_string(),
            mood: "neutral".to_string(),
            salience: 7,
            summary: Some("Imported document".to_string()),
            memory_type: "fact".to_string(),
            tags: vec!["document".to_string(), "imported".to_string()],
            intent: "document_import".to_string(),
            monologue: None,
            reasoning_summary: None,
            aside_intensity: None,
        };

        self.memory_service
            .evaluate_and_save_response("document-import", &doc_response, None)
            .await?;

        Ok(())
    }

    async fn process_for_project_vector_store(
        &self,
        content: &str,
        project_id: Option<&str>,
    ) -> Result<()> {
        let doc_response = MiraStructuredReply {
            output: content.to_string(),
            persona: "system".to_string(),
            mood: "neutral".to_string(),
            salience: 7,
            summary: Some("Imported project document".to_string()),
            memory_type: "fact".to_string(),
            tags: vec!["document".to_string(), "imported".to_string(), "project".to_string()],
            intent: "project_document_import".to_string(),
            monologue: None,
            reasoning_summary: None,
            aside_intensity: None,
        };

        self.memory_service
            .evaluate_and_save_response("document-import", &doc_response, project_id)
            .await?;

        Ok(())
    }
}
