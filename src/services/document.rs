// src/services/document.rs

use crate::llm::assistant::VectorStoreManager;
use crate::services::{MemoryService, ChatService};
use crate::llm::schema::MiraStructuredReply;
use std::sync::Arc;
use std::path::Path;
use anyhow::{Result, Context};
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Copy)]
pub enum DocumentDestination {
    PersonalMemory,      // Goes to Qdrant
    ProjectVectorStore,  // Goes to OpenAI
    Both,                // Hybrid storage
}

#[derive(Serialize)]
#[allow(dead_code)]
struct RoutingPrompt<'a> {
    file_name: &'a str,
    preview: &'a str,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct RoutingResponse {
    destination: String,
}

pub struct DocumentService {
    vector_store_manager: Arc<VectorStoreManager>,
    memory_service: Arc<MemoryService>,
    chat_service: Arc<ChatService>,
}

impl DocumentService {
    pub fn new(
        vector_store_manager: Arc<VectorStoreManager>,
        memory_service: Arc<MemoryService>,
        chat_service: Arc<ChatService>,
    ) -> Self {
        Self {
            vector_store_manager,
            memory_service,
            chat_service,
        }
    }

    pub async fn process_document(
        &self,
        file_path: &Path,
        content: &str,
        project_id: Option<&str>,
    ) -> Result<()> {
        let destination = self.analyze_and_route(file_path, content).await?;

        match destination {
            DocumentDestination::PersonalMemory => {
                self.process_for_personal_memory(content).await?;
            }
            DocumentDestination::ProjectVectorStore => {
                if let Some(proj_id) = project_id {
                    self.vector_store_manager
                        .add_document(proj_id, file_path.to_path_buf())
                        .await
                        .context("Failed to add document to OpenAI vector store")?;
                } else {
                    return Err(anyhow::anyhow!("Project ID required for vector store upload"));
                }
            }
            DocumentDestination::Both => {
                if let Some(proj_id) = project_id {
                    self.vector_store_manager
                        .add_document(proj_id, file_path.to_path_buf())
                        .await
                        .context("Failed to add document to OpenAI vector store (Both)")?;
                } else {
                    return Err(anyhow::anyhow!("Project ID required for vector store upload (Both)"));
                }
                self.process_for_personal_memory(content).await?;
            }
        }

        Ok(())
    }

    /// Analyze document and determine routing destination
    async fn analyze_and_route(
        &self,
        file_path: &Path,
        content: &str,
    ) -> Result<DocumentDestination> {
        let extension = file_path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        
        let file_name = file_path.file_name()
            .and_then(|f| f.to_str())
            .unwrap_or("document");
        
        // Simple heuristic-based routing (can be enhanced with LLM later)
        
        // Personal notes patterns
        if file_name.contains("diary") || 
           file_name.contains("personal") ||
           file_name.contains("journal") ||
           content.contains("diary") || 
           content.contains("personal journal") {
            return Ok(DocumentDestination::PersonalMemory);
        }
        
        // Technical documentation patterns
        if extension == "md" || 
           extension == "pdf" || 
           extension == "txt" ||
           extension == "rs" ||
           extension == "js" ||
           extension == "py" {
            // Check if it has personal markers
            if content.len() < 1000 && content.contains("personal") {
                return Ok(DocumentDestination::PersonalMemory);
            }
            return Ok(DocumentDestination::ProjectVectorStore);
        }
        
        // Important mixed content
        if content.len() > 5000 && 
           (content.contains("insight") || content.contains("reflection")) {
            return Ok(DocumentDestination::Both);
        }
        
        // Default to project store
        Ok(DocumentDestination::ProjectVectorStore)
    }

    /// LLM-powered routing (optional enhancement)
    #[allow(dead_code)]
    async fn llm_analyze_and_route(
        &self,
        file_path: &Path,
        content: &str,
    ) -> Result<DocumentDestination> {
        let file_name = file_path.file_name().and_then(|f| f.to_str()).unwrap_or("document");
        let preview = &content[..content.len().min(2048)];

        let system_prompt = "You are a smart assistant that decides where to store user documents. \
        Return ONLY one of: PersonalMemory, ProjectVectorStore, or Both. \
        PersonalMemory = user's private, personal, or diary-like notes. \
        ProjectVectorStore = technical docs, code, PDFs, or project files. \
        Both = big docs that are both personal and important to a project.";

        let user_prompt = format!(
            "File name: {}\nPreview: {}\nWhere should this document be routed?",
            file_name, preview
        );

        // This would need a proper inference method in ChatService
        // For now, we'll use the heuristic method above
        let _ = (system_prompt, user_prompt); // Suppress unused warnings
        
        // Fallback to heuristic routing
        self.analyze_and_route(file_path, content).await
    }

    /// Process document content for personal memory storage
    async fn process_for_personal_memory(&self, content: &str) -> Result<()> {
        // Create a MiraStructuredReply to save through MemoryService
        let doc_response = MiraStructuredReply {
            output: content.to_string(),
            persona: "system".to_string(),
            mood: "neutral".to_string(),
            salience: 7, // High salience for documents
            summary: Some("Imported document".to_string()),
            memory_type: "fact".to_string(),
            tags: vec!["document".to_string(), "imported".to_string()],
            intent: "document_import".to_string(),
            monologue: None,
            reasoning_summary: None,
            aside_intensity: None,
        };
        
        // Use the memory service's evaluate_and_save_response method
        self.memory_service.evaluate_and_save_response(
            "document-import",
            &doc_response,
            None, // No project_id for personal memory
        ).await?;
        
        Ok(())
    }
}
