// src/tools/document.rs

use anyhow::Result;
use std::path::Path;
use std::sync::Arc;

use crate::llm::responses::VectorStoreManager;
use crate::llm::types::ChatResponse;
use crate::memory::MemoryService;

#[derive(Debug, Clone, Copy)]
pub enum DocumentDestination {
    PersonalMemory,
    ProjectVectorStore,
    Both,
}

pub struct DocumentService {
    memory_service: Arc<MemoryService>,
    vector_store_manager: Arc<VectorStoreManager>,
}

impl DocumentService {
    pub fn new(
        memory_service: Arc<MemoryService>,
        vector_store_manager: Arc<VectorStoreManager>,
    ) -> Self {
        Self {
            memory_service,
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
                let store_key = project_id
                    .expect("Project ID required for project vector store upload");
                self.vector_store_manager
                    .add_document(store_key, file_path.to_path_buf())
                    .await?;
                self.process_for_project_vector_store(content, project_id).await?;
            }
            DocumentDestination::Both => {
                let store_key = project_id
                    .expect("Project ID required for vector store upload (Both)");
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
        let extension = file_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        let file_name = file_path
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or("document")
            .to_ascii_lowercase();

        if file_name.contains("diary")
            || file_name.contains("personal")
            || file_name.contains("journal")
            || content.to_ascii_lowercase().contains("dear diary")
            || content.to_ascii_lowercase().contains("personal journal")
        {
            return Ok(DocumentDestination::PersonalMemory);
        }

        let technical_exts = ["md", "pdf", "txt", "rs", "js", "py"];
        if technical_exts.contains(&extension.as_str()) {
            if project_id.is_none() {
                return Err(anyhow::anyhow!(
                    "Technical documents require a project ID for vector storage"
                ));
            }
            return Ok(DocumentDestination::ProjectVectorStore);
        }

        if content.len() > 5000
            && (content.contains("insight") || content.contains("reflection"))
        {
            if project_id.is_none() {
                return Err(anyhow::anyhow!(
                    "Both destination requires a project ID for vector storage"
                ));
            }
            return Ok(DocumentDestination::Both);
        }

        if project_id.is_some() {
            Ok(DocumentDestination::ProjectVectorStore)
        } else {
            Err(anyhow::anyhow!(
                "Project ID required for project vector store upload"
            ))
        }
    }

    async fn process_for_personal_memory(&self, content: &str) -> Result<()> {
        let doc_response = ChatResponse {
            output: content.to_string(),
            persona: "system".to_string(),
            mood: "neutral".to_string(),
            salience: 7,
            summary: "Imported document".to_string(),
            memory_type: "fact".to_string(),
            tags: vec!["document".into(), "imported".into()],
            intent: Some("document_import".to_string()),
            monologue: None,
            reasoning_summary: None,
        };

        self.memory_service
            .save_assistant_response("document-import", &doc_response)
            .await?;

        Ok(())
    }

    async fn process_for_project_vector_store(
        &self,
        content: &str,
        project_id: Option<&str>,
    ) -> Result<()> {
        let doc_response = ChatResponse {
            output: content.to_string(),
            persona: "system".to_string(),
            mood: "neutral".to_string(),
            salience: 7,
            summary: "Imported project document".to_string(),
            memory_type: "fact".to_string(),
            tags: vec!["document".into(), "imported".into(), "project".into()],
            intent: Some("project_document_import".to_string()),
            monologue: None,
            reasoning_summary: None,
        };

        let session_id = if let Some(pid) = project_id {
            format!("document-import-{pid}")
        } else {
            "document-import".to_string()
        };

        self.memory_service
            .save_assistant_response(&session_id, &doc_response)
            .await?;

        Ok(())
    }
}
