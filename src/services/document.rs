// src/services/document.rs

use crate::llm::assistant::VectorStoreManager;
use crate::services::{MemoryService, ChatService};
use std::sync::Arc;
use std::path::{Path, PathBuf};
use anyhow::{Result, Context};
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Copy)]
pub enum DocumentDestination {
    PersonalMemory,      // Goes to Qdrant
    ProjectVectorStore,  // Goes to OpenAI
    Both,                // Hybrid storage
}

#[derive(Serialize)]
struct RoutingPrompt<'a> {
    file_name: &'a str,
    preview: &'a str,
}

#[derive(Deserialize)]
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
        let destination = self.llm_analyze_and_route(file_path, content).await?;

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

    /// LLM-powered, robust routing based on file name and preview.
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

        // Use your existing LLM/chat pipeline.
        let routing_raw = self.chat_service
            .run_routing_inference(system_prompt, &user_prompt)
            .await?;

        // Expect a clean answer: "PersonalMemory", "ProjectVectorStore", or "Both"
        let routing = routing_raw.trim().replace('\"', "");

        match routing.as_str() {
            "PersonalMemory" => Ok(DocumentDestination::PersonalMemory),
            "ProjectVectorStore" => Ok(DocumentDestination::ProjectVectorStore),
            "Both" => Ok(DocumentDestination::Both),
            other => Err(anyhow::anyhow!("Invalid routing decision: {}", other)),
        }
    }

    /// Example: process for Qdrant (personal memory)
    async fn process_for_personal_memory(&self, content: &str) -> Result<()> {
        self.memory_service.ingest_personal_note(content).await?;
        Ok(())
    }
}
