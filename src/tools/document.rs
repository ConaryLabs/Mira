// src/tools/document.rs

use anyhow::Result;
use std::path::Path;
use std::sync::Arc;

use crate::llm::types::ChatResponse;
use crate::memory::MemoryService;
use crate::memory::storage::qdrant::multi_store::QdrantMultiStore;
use crate::llm::embeddings::EmbeddingHead;
use crate::memory::core::types::MemoryEntry;
use chrono::Utc;

#[derive(Debug, Clone, Copy)]
pub enum DocumentDestination {
    PersonalMemory,
    ProjectDocuments,
    Both,
}

pub struct DocumentService {
    memory_service: Arc<MemoryService>,
    multi_store: Arc<QdrantMultiStore>,
}

impl DocumentService {
    pub fn new(
        memory_service: Arc<MemoryService>,
        multi_store: Arc<QdrantMultiStore>,
    ) -> Self {
        Self {
            memory_service,
            multi_store,
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
            DocumentDestination::ProjectDocuments => {
                let project_id = project_id
                    .ok_or_else(|| anyhow::anyhow!("Project ID required for project documents"))?;
                self.store_in_qdrant(file_path, content, project_id).await?;
            }
            DocumentDestination::Both => {
                let project_id = project_id
                    .ok_or_else(|| anyhow::anyhow!("Project ID required for Both destination"))?;
                self.store_in_qdrant(file_path, content, project_id).await?;
                self.process_for_personal_memory(content).await?;
            }
        }

        Ok(())
    }

    async fn store_in_qdrant(
        &self,
        file_path: &Path,
        content: &str,
        project_id: &str,
    ) -> Result<()> {
        // Create memory entry for document using tags for metadata
        let file_name = file_path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");
        
        let extension = file_path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        
        let entry = MemoryEntry {
            id: None,
            session_id: format!("project-{}", project_id),
            response_id: None,
            parent_id: None,
            role: "document".to_string(),
            content: content.to_string(),
            timestamp: Utc::now(),
            tags: Some(vec![
                "document".to_string(),
                "imported".to_string(),
                format!("file:{}", file_path.to_string_lossy()),
                format!("name:{}", file_name),
                format!("ext:{}", extension),
                format!("project:{}", project_id),
            ]),
            
            // Analysis fields
            mood: None,
            intensity: None,
            salience: Some(7.0),  // FIXED: changed to f32
            intent: Some("document_import".to_string()),
            topics: None,
            summary: Some(format!("Document: {}", file_name)),
            relationship_impact: None,
            contains_code: None,
            language: None,
            programming_lang: None,
            analyzed_at: None,
            analysis_version: None,
            routed_to_heads: None,
            last_recalled: None,
            recall_count: None,
            
            // GPT5 metadata fields
            model_version: None,
            prompt_tokens: None,
            completion_tokens: None,
            reasoning_tokens: None,
            total_tokens: None,
            latency_ms: None,
            generation_time_ms: None,
            finish_reason: None,
            tool_calls: None,
            temperature: None,
            max_tokens: None,
            reasoning_effort: None,
            verbosity: None,
            
            // Embedding info
            embedding: None,
            embedding_heads: Some(vec!["documents".to_string()]),
            qdrant_point_ids: None,
        };

        // Store in Documents collection
        self.multi_store
            .save(EmbeddingHead::Documents, &entry)
            .await?;

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

        // Personal documents go to memory
        if file_name.contains("diary")
            || file_name.contains("personal")
            || file_name.contains("journal")
            || content.to_ascii_lowercase().contains("dear diary")
            || content.to_ascii_lowercase().contains("personal journal")
        {
            return Ok(DocumentDestination::PersonalMemory);
        }

        // Technical documents go to Qdrant Documents
        let technical_exts = ["md", "pdf", "txt", "rs", "js", "py"];
        if technical_exts.contains(&extension.as_str()) {
            if project_id.is_none() {
                return Err(anyhow::anyhow!(
                    "Technical documents require a project ID"
                ));
            }
            return Ok(DocumentDestination::ProjectDocuments);
        }

        // Large reflective documents go to both
        if content.len() > 5000
            && (content.contains("insight") || content.contains("reflection"))
        {
            if project_id.is_none() {
                return Err(anyhow::anyhow!(
                    "Both destination requires a project ID"
                ));
            }
            return Ok(DocumentDestination::Both);
        }

        // Default to project documents if project_id exists
        if project_id.is_some() {
            Ok(DocumentDestination::ProjectDocuments)
        } else {
            Ok(DocumentDestination::PersonalMemory)
        }
    }

    async fn process_for_personal_memory(&self, content: &str) -> Result<()> {
        let doc_response = ChatResponse {
            output: content.to_string(),
            persona: "system".to_string(),
            mood: "neutral".to_string(),
            salience: 7.0,  // FIXED: changed to f32
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
}
