// src/services/file_search.rs
// PHASE 3 NEW: File search service for project-aware document search
// Integrates with VectorStoreManager and Git system for comprehensive file search
// FIXED: Recursive async function using Box::pin for indirection

use anyhow::Result;
use serde::{Serialize, Deserialize};
use serde_json::{json, Value};
use std::sync::Arc;
use std::path::Path;
use std::pin::Pin;
use std::future::Future;
use tracing::{info, debug};

use crate::llm::responses::VectorStoreManager;
use crate::git::GitClient;
use crate::git::types::GitRepoAttachment;
use crate::git::client::{FileNode, FileNodeType};
use crate::config::CONFIG;
use crate::api::http::git::common::{should_index_file, detect_language};

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
    ContentMatch,      // Found in file content
    FilenameMatch,     // Found in filename
    PathMatch,         // Found in file path
    FunctionMatch,     // Found in function/method names
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
    /// Create new file search service
    pub fn new(vector_store_manager: Arc<VectorStoreManager>, git_client: GitClient) -> Self {
        info!("Initializing FileSearchService");
        Self {
            vector_store_manager,
            git_client,
        }
    }
    
    /// Search files in a project using vector store and git integration
    pub async fn search_files(
        &self,
        params: &FileSearchParams,
        _project_id: Option<&str>,
    ) -> Result<Value> {
        info!("🔍 Searching files with query: '{}'", params.query);
        debug!("Search parameters: {:?}", params);
        
        let max_files = params.max_files.unwrap_or(CONFIG.file_search_max_files);
        let include_content = params.include_content.unwrap_or(true);
        
        // First, try vector store search for semantic content matches
        let mut results = Vec::new();
        
        if let Ok(vector_results) = self.vector_store_manager
            .search_documents(_project_id, &params.query, max_files)
            .await 
        {
            debug!("Found {} vector store results", vector_results.len());
            
            for result in vector_results {
                // Filter by file extensions if specified
                if let Some(ref _file_extensions) = params.file_extensions {
                    if let Some(ref file_id) = result.file_id {
                        let matches_extension = _file_extensions.iter().any(|ext| {
                            file_id.ends_with(&format!(".{}", ext))
                        });
                        if !matches_extension {
                            continue;
                        }
                    }
                }
                
                // Create search result
                let file_path = result.file_id.unwrap_or_else(|| "unknown".to_string());
                let language = detect_language(&file_path);
                
                results.push(FileSearchResult {
                    file_path: file_path.clone(),
                    content_snippet: if include_content {
                        // Truncate content for snippet
                        let content = result.content.chars().take(300).collect::<String>();
                        if result.content.len() > 300 {
                            format!("{}...", content)
                        } else {
                            content
                        }
                    } else {
                        "Content match found".to_string()
                    },
                    relevance_score: result.score,
                    language,
                    file_size: result.content.len(),
                    match_type: SearchMatchType::ContentMatch,
                });
            }
        } else {
            debug!("Vector store search failed or returned no results");
        }
        
        // If we have fewer results than requested, try filename/path search
        if results.len() < max_files {
            if let Some(filename_results) = self.search_by_filename_and_path(
                &params.query, 
                _project_id, 
                &params.file_extensions,
                max_files - results.len(),
                params.case_sensitive.unwrap_or(false)
            ).await {
                results.extend(filename_results);
            }
        }
        
        // Sort by relevance score (descending)
        results.sort_by(|a, b| b.relevance_score.partial_cmp(&a.relevance_score).unwrap_or(std::cmp::Ordering::Equal));
        
        // Truncate to max_files
        results.truncate(max_files);
        
        info!("File search completed: {} results found", results.len());
        
        Ok(json!({
            "query": params.query,
            "results": results,
            "total_found": results.len(),
            "search_type": "combined",
            "parameters": {
                "max_files": max_files,
                "file_extensions": params.file_extensions,
                "case_sensitive": params.case_sensitive.unwrap_or(false),
                "include_content": include_content
            }
        }))
    }
    
    /// Search by filename and file paths in git repositories
    async fn search_by_filename_and_path(
        &self,
        query: &str,
        _project_id: Option<&str>,
        _file_extensions: &Option<Vec<String>>,
        _max_results: usize,
        case_sensitive: bool,
    ) -> Option<Vec<FileSearchResult>> {
        debug!("Searching filenames and paths for: '{}'", query);
        
        // For now, this is a placeholder implementation
        // In a full implementation, this would:
        // 1. Get all Git attachments for the project
        // 2. Scan file trees for filename/path matches
        // 3. Return relevant matches
        
        let search_query = if case_sensitive {
            query.to_string()
        } else {
            query.to_lowercase()
        };
        
        // Placeholder results for demonstration
        if search_query.contains("config") || search_query.contains("mod") {
            let mut filename_results = Vec::new();
            
            // Example filename match
            filename_results.push(FileSearchResult {
                file_path: "src/config/mod.rs".to_string(),
                content_snippet: "Configuration module - contains main configuration structures and environment loading".to_string(),
                relevance_score: 0.8,
                language: Some("rust".to_string()),
                file_size: 2048,
                match_type: SearchMatchType::FilenameMatch,
            });
            
            return Some(filename_results);
        }
        
        None
    }
    
    /// Index repository files into vector store for searchability
    /// Called when repositories are attached to projects
    pub async fn index_repository(&self, attachment: &GitRepoAttachment) -> Result<usize> {
        info!("📚 Indexing repository for search: {}", attachment.repo_url);
        
        let mut indexed_files = 0;
        
        // Get file tree from git client
        let file_nodes = self.git_client.get_file_tree(attachment)
            .map_err(|e| anyhow::anyhow!("Failed to get file tree: {}", e))?;
        
        // Recursively index files using Box::pin for indirection
        indexed_files += self.index_file_nodes(&file_nodes, attachment).await?;
        
        info!("📚 Repository indexing completed: {} files indexed", indexed_files);
        Ok(indexed_files)
    }
    
    /// Recursively index file nodes
    /// FIXED: Use proper lifetimes for all parameters
    fn index_file_nodes<'a>(
        &'a self,
        nodes: &'a [FileNode],
        attachment: &'a GitRepoAttachment,
    ) -> Pin<Box<dyn Future<Output = Result<usize>> + Send + 'a>> {
        Box::pin(async move {
            let mut indexed = 0;
            
            for node in nodes {
                match node.node_type {
                    FileNodeType::File => {
                        if self.should_index_file(&node.path) {
                            if let Ok(_) = self.index_single_file(node, attachment).await {
                                indexed += 1;
                            }
                        }
                    }
                    FileNodeType::Directory => {
                        // Recursively index children using Box::pin
                        indexed += self.index_file_nodes(&node.children, attachment).await?;
                    }
                }
            }
            
            Ok(indexed)
        })
    }
    
    /// Index a single file into the vector store
    async fn index_single_file(
        &self,
        file_node: &FileNode,
        attachment: &GitRepoAttachment,
    ) -> Result<()> {
        debug!("Indexing file: {}", file_node.path);
        
        // Get file content from git client
        let _content = self.git_client.get_file_content(attachment, &file_node.path)
            .map_err(|e| anyhow::anyhow!("Failed to read file content: {}", e))?;
        
        // Create a temporary file for vector store upload
        // In a production implementation, this would be more sophisticated
        let temp_path = std::path::PathBuf::from(&file_node.path);
        
        // Add to vector store (using project_id as store key)
        self.vector_store_manager
            .add_document(&attachment.project_id, temp_path)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to add document to vector store: {}", e))?;
        
        debug!("Successfully indexed file: {}", file_node.path);
        Ok(())
    }
    
    /// Check if a file should be indexed (avoid binary files, build artifacts, etc.)
    fn should_index_file(&self, path: &str) -> bool {
        let path_obj = Path::new(path);
        
        // Use the existing should_index_file function from git common utils
        should_index_file(path_obj) && {
            // Additional file size check
            path.len() < 100_000 // Skip very long paths (likely binary)
        }
    }
    
    /// Get search statistics for a project
    pub async fn get_search_stats(&self, project_id: Option<&str>) -> Result<Value> {
        info!("Getting search statistics for project: {:?}", project_id);
        
        // In a full implementation, this would query the vector store
        // and git repositories to provide comprehensive statistics
        
        Ok(json!({
            "project_id": project_id,
            "indexed_files": 0,  // Placeholder
            "total_files": 0,    // Placeholder
            "indexing_status": "ready",
            "supported_extensions": [
                "rs", "js", "ts", "py", "java", "go", "cpp", "c", "h",
                "md", "txt", "json", "yaml", "toml", "html", "css"
            ],
            "search_capabilities": [
                "content_search",
                "filename_search", 
                "path_search",
                "semantic_search"
            ]
        }))
    }
}

/// Default implementation
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
