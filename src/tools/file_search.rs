// src/tools/file_search.rs
use anyhow::Result;
use serde::{Serialize, Deserialize};
use serde_json::{json, Value};
use std::sync::Arc;
use tracing::info;
use crate::memory::storage::qdrant::multi_store::QdrantMultiStore;
use crate::llm::client::OpenAIClient;
use crate::llm::embeddings::EmbeddingHead;

#[derive(Clone)]
pub struct FileSearchService {
    multi_store: Arc<QdrantMultiStore>,
    llm_client: Arc<OpenAIClient>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSearchResult {
    pub file_path: String,
    pub content_snippet: String,
    pub relevance_score: f32,
    pub language: Option<String>,
    pub file_size: usize,
    pub match_type: SearchMatchType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SearchMatchType {
    ContentMatch,
    FilenameMatch,
    PathMatch,
    FunctionMatch,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSearchParams {
    pub query: String,
    pub file_extensions: Option<Vec<String>>,
    pub max_files: Option<usize>,
    pub case_sensitive: Option<bool>,
    pub include_content: Option<bool>,
}

impl FileSearchService {
    pub fn new(multi_store: Arc<QdrantMultiStore>, llm_client: Arc<OpenAIClient>) -> Self {
        info!("Initializing Qdrant-based FileSearchService");
        Self {
            multi_store,
            llm_client,
        }
    }
    
    pub async fn search_files(
        &self,
        params: &FileSearchParams,
        project_id: Option<&str>,
    ) -> Result<Value> {
        info!("Searching files with query: '{}'", params.query);
        
        // Hardcode max_files default
        let max_files = params.max_files.unwrap_or(50);
        
        // Generate embedding for the search query
        let query_embedding = self.llm_client
            .get_embedding(&params.query)
            .await?;
        
        // Build session ID for scoped search
        let session_id = project_id
            .map(|pid| format!("project-{}", pid))
            .unwrap_or_else(|| "global".to_string());
        
        // Search in Documents collection
        let search_results = self.multi_store
            .search(
                EmbeddingHead::Documents,
                &session_id,
                &query_embedding,
                max_files,
            )
            .await?;
        
        // Convert to FileSearchResult format
        let results: Vec<FileSearchResult> = search_results
            .into_iter()
            .map(|entry| {
                // Extract file path from tags
                let file_path = entry.tags
                    .as_ref()
                    .and_then(|tags| {
                        tags.iter()
                            .find(|t| t.starts_with("file:"))
                            .and_then(|t| t.strip_prefix("file:"))
                    })
                    .unwrap_or("unknown")
                    .to_string();
                
                // Extract language from tags
                let language = entry.tags
                    .as_ref()
                    .and_then(|tags| {
                        tags.iter()
                            .find(|t| t.starts_with("lang:"))
                            .and_then(|t| t.strip_prefix("lang:"))
                    })
                    .map(|s| s.to_string());
                
                FileSearchResult {
                    file_path,
                    content_snippet: entry.content.chars().take(300).collect(),
                    relevance_score: entry.salience.unwrap_or(0.0),
                    language,
                    file_size: entry.content.len(),
                    match_type: SearchMatchType::ContentMatch,
                }
            })
            .collect();
        
        Ok(json!({
            "query": params.query,
            "results": results,
            "total_found": results.len(),
        }))
    }
}

impl Default for FileSearchParams {
    fn default() -> Self {
        Self {
            query: String::new(),
            file_extensions: None,
            max_files: Some(50),  // Hardcoded default
            case_sensitive: Some(false),
            include_content: Some(true),
        }
    }
}
