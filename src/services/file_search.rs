// src/services/file_search.rs
// File search service for project-aware document search

use anyhow::Result;
use serde::{Serialize, Deserialize};
use serde_json::{json, Value};
use std::sync::Arc;
use tracing::info;

use crate::llm::responses::VectorStoreManager;
use crate::git::GitClient;
use crate::config::CONFIG;

/// File search service for finding content within project repositories
#[derive(Clone)]
pub struct FileSearchService {
    vector_store_manager: Arc<VectorStoreManager>,
    git_client: GitClient,
}

/// Search result for individual files
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSearchResult {
    pub file_path: String,
    pub content_snippet: String,
    pub relevance_score: f32,
    pub language: Option<String>,
    pub file_size: usize,
    pub match_type: SearchMatchType,
}

/// Type of search match found
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SearchMatchType {
    ContentMatch,
    FilenameMatch,
    PathMatch,
    FunctionMatch,
}

/// Parameters for file search requests
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSearchParams {
    pub query: String,
    pub file_extensions: Option<Vec<String>>,
    pub max_files: Option<usize>,
    pub case_sensitive: Option<bool>,
    pub include_content: Option<bool>,
}

impl FileSearchService {
    pub fn new(vector_store_manager: Arc<VectorStoreManager>, git_client: GitClient) -> Self {
        info!("Initializing FileSearchService");
        Self {
            vector_store_manager,
            git_client,
        }
    }
    
    /// Search files - minimal implementation to satisfy the executor
    pub async fn search_files(
        &self,
        params: &FileSearchParams,
        project_id: Option<&str>,
    ) -> Result<Value> {
        info!("Searching files with query: '{}'", params.query);
        
        let max_files = params.max_files.unwrap_or(CONFIG.file_search_max_files);
        
        // Try vector store search
        let results = if let Ok(vector_results) = self.vector_store_manager
            .search_documents(project_id, &params.query, max_files)
            .await 
        {
            vector_results.into_iter()
                .map(|result| FileSearchResult {
                    file_path: result.file_id.unwrap_or_else(|| "unknown".to_string()),
                    content_snippet: result.content.chars().take(300).collect(),
                    relevance_score: result.score,
                    language: None,
                    file_size: result.content.len(),
                    match_type: SearchMatchType::ContentMatch,
                })
                .collect()
        } else {
            Vec::new()
        };
        
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
            max_files: Some(CONFIG.file_search_max_files),
            case_sensitive: Some(false),
            include_content: Some(true),
        }
    }
}
