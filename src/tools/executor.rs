// src/tools/executor.rs
use anyhow::Result;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::sync::Arc;
use std::path::Path;
use tracing::{info, debug};

use crate::memory::features::code_intelligence::CodeIntelligenceService;

pub struct ToolExecutor {
    code_intelligence: Arc<CodeIntelligenceService>,
    sqlite_pool: SqlitePool,
}

impl ToolExecutor {
    pub fn new(code_intelligence: Arc<CodeIntelligenceService>, sqlite_pool: SqlitePool) -> Self {
        Self {
            code_intelligence,
            sqlite_pool,
        }
    }

    /// Execute a tool by name
    pub async fn execute_tool(&self, tool_name: &str, input: &Value, project_id: &str) -> Result<Value> {
        match tool_name {
            "read_file" => self.execute_read_file(input, project_id).await,
            "search_code" => self.execute_search_code(input, project_id).await,
            "list_files" => self.execute_list_files(input, project_id).await,
            _ => Err(anyhow::anyhow!("Unknown tool: {}", tool_name)),
        }
    }

    async fn execute_read_file(&self, input: &Value, project_id: &str) -> Result<Value> {
        let path = input.get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' parameter"))?;

        info!("Reading file: {}", path);
        
        let content = super::file_ops::load_complete_file(&self.sqlite_pool, path, project_id).await?;
        
        Ok(json!({
            "path": path,
            "content": content,
            "lines": content.lines().count()
        }))
    }

    async fn execute_search_code(&self, input: &Value, project_id: &str) -> Result<Value> {
        let query = input.get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'query' parameter"))?;

        info!("Searching code: {}", query);
        
        let results = self.code_intelligence
            .search_elements_for_project(query, project_id, Some(20))
            .await?;

        Ok(json!({
            "query": query,
            "results": results,
            "count": results.len()
        }))
    }

    async fn execute_list_files(&self, input: &Value, project_id: &str) -> Result<Value> {
        let path = input.get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        info!("Listing files in: {}", if path.is_empty() { "root" } else { path });
        
        // Try project repo first
        if let Some(entries) = self.try_list_project_repo(path, project_id).await? {
            return Ok(json!({
                "path": path,
                "entries": entries
            }));
        }

        // Fallback: List from backend working directory
        debug!("Listing from backend working directory");
        let backend_path = Path::new(path);
        let dir_path = if path.is_empty() || path == "." {
            std::env::current_dir()?
        } else {
            backend_path.to_path_buf()
        };

        let mut entries = Vec::new();
        let mut dir = tokio::fs::read_dir(&dir_path).await?;
        
        while let Some(entry) = dir.next_entry().await? {
            let metadata = entry.metadata().await?;
            let name = entry.file_name().to_string_lossy().to_string();
            
            entries.push(json!({
                "name": name,
                "is_file": metadata.is_file(),
                "is_dir": metadata.is_dir(),
                "size": metadata.len()
            }));
        }

        Ok(json!({
            "path": path,
            "entries": entries,
            "source": "backend_directory"
        }))
    }

    /// Try to list files from project's git repository
    async fn try_list_project_repo(&self, path: &str, project_id: &str) -> Result<Option<Vec<Value>>> {
        // Get git attachment for project
        let attachment = match sqlx::query!(
            r#"SELECT local_path FROM git_repo_attachments WHERE project_id = ? LIMIT 1"#,
            project_id
        )
        .fetch_optional(&self.sqlite_pool)
        .await? {
            Some(att) => att,
            None => {
                debug!("No git attachment for project {}, will list backend directory", project_id);
                return Ok(None);
            }
        };

        let base_path = Path::new(&attachment.local_path).join(path);
        
        // Try to read directory
        let mut dir = match tokio::fs::read_dir(&base_path).await {
            Ok(d) => d,
            Err(e) => {
                debug!("Failed to list project repo directory {}: {}", base_path.display(), e);
                return Ok(None);  // Trigger fallback
            }
        };
        
        let mut entries = Vec::new();
        while let Some(entry) = dir.next_entry().await? {
            let metadata = entry.metadata().await?;
            let name = entry.file_name().to_string_lossy().to_string();
            
            entries.push(json!({
                "name": name,
                "is_file": metadata.is_file(),
                "is_dir": metadata.is_dir(),
                "size": metadata.len()
            }));
        }

        Ok(Some(entries))
    }
}
