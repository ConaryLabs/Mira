// src/tools/executor.rs
// Tool executor providing access to file operations, code search, and project context

use anyhow::Result;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::sync::Arc;
use std::path::PathBuf;
use tracing::info;

use crate::memory::features::code_intelligence::CodeIntelligenceService;

pub struct ToolExecutor {
    code_intelligence: Arc<CodeIntelligenceService>,
    sqlite_pool: SqlitePool,
}

impl ToolExecutor {
    pub fn new(
        code_intelligence: Arc<CodeIntelligenceService>,
        sqlite_pool: SqlitePool,
    ) -> Self {
        Self {
            code_intelligence,
            sqlite_pool,
        }
    }

    /// Execute a tool by name
    pub async fn execute_tool(&self, tool_name: &str, input: &Value, project_id: &str) -> Result<Value> {
        match tool_name {
            "create_artifact" => self.execute_create_artifact(input).await,
            "read_file" => self.execute_read_file(input, project_id).await,
            "list_files" => self.execute_list_files(input, project_id).await,
            "read_files" => self.execute_read_files(input, project_id).await,
            "write_files" => self.execute_write_files(input, project_id).await,
            "search_code" => self.execute_search_code(input, project_id).await,
            "get_project_context" => self.execute_project_context(input, project_id).await,
            _ => Err(anyhow::anyhow!("Unknown tool: {}", tool_name)),
        }
    }

    async fn execute_create_artifact(&self, input: &Value) -> Result<Value> {
        let title = input.get("title")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'title' parameter"))?;
        
        let content = input.get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'content' parameter"))?;
        
        let language = input.get("language")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'language' parameter"))?;
        
        let path = input.get("path")
            .and_then(|v| v.as_str());
        
        info!("Creating artifact: {} ({})", title, language);
        
        Ok(json!({
            "type": "code",
            "artifacts": [{
                "title": title,
                "content": content,
                "language": language,
                "path": path,
                "lines": content.lines().count(),
            }],
            "message": format!("Created artifact: {}", title)
        }))
    }

    async fn execute_read_file(&self, input: &Value, project_id: &str) -> Result<Value> {
        let path = input.get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' parameter"))?;
        
        let content = super::file_ops::load_complete_file(&self.sqlite_pool, path, project_id).await?;
        
        Ok(json!({
            "path": path,
            "content": content,
            "lines": content.lines().count()
        }))
    }

    async fn execute_list_files(&self, input: &Value, project_id: &str) -> Result<Value> {
        let directory = input.get("directory")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        
        let repo_path = self.get_repo_path(project_id).await?;
        let full_path = repo_path.join(directory);
        
        if !full_path.exists() {
            return Ok(json!({
                "directory": directory,
                "files": [],
                "total": 0
            }));
        }
        
        let mut files = Vec::new();
        if let Ok(entries) = tokio::fs::read_dir(&full_path).await {
            let mut entries = entries;
            while let Ok(Some(entry)) = entries.next_entry().await {
                if let Ok(metadata) = entry.metadata().await {
                    let file_name = entry.file_name().to_string_lossy().to_string();
                    files.push(json!({
                        "name": file_name,
                        "is_dir": metadata.is_dir(),
                        "size": metadata.len()
                    }));
                }
            }
        }
        
        Ok(json!({
            "directory": directory,
            "files": files,
            "total": files.len()
        }))
    }

    async fn execute_read_files(&self, input: &Value, project_id: &str) -> Result<Value> {
        let paths = input.get("paths")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow::anyhow!("Missing 'paths' parameter or not an array"))?;
        
        let mut results = Vec::new();
        
        for path_value in paths {
            let path = path_value.as_str()
                .ok_or_else(|| anyhow::anyhow!("Path must be a string"))?;
            
            match super::file_ops::load_complete_file(&self.sqlite_pool, path, project_id).await {
                Ok(content) => {
                    results.push(json!({
                        "path": path,
                        "content": content,
                        "lines": content.lines().count(),
                        "success": true
                    }));
                }
                Err(e) => {
                    results.push(json!({
                        "path": path,
                        "error": e.to_string(),
                        "success": false
                    }));
                }
            }
        }
        
        Ok(json!({
            "files": results,
            "total": paths.len(),
            "successful": results.iter().filter(|r| r["success"].as_bool().unwrap_or(false)).count()
        }))
    }

    async fn execute_write_files(&self, input: &Value, project_id: &str) -> Result<Value> {
        let files = input.get("files")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow::anyhow!("Missing 'files' parameter or not an array"))?;
        
        let repo_path = self.get_repo_path(project_id).await?;
        let mut results = Vec::new();
        
        for file in files {
            let path = file.get("path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'path' in file object"))?;
            
            let content = file.get("content")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'content' in file object"))?;
            
            let full_path = repo_path.join(path);
            
            if let Some(parent) = full_path.parent() {
                if let Err(e) = tokio::fs::create_dir_all(parent).await {
                    results.push(json!({
                        "path": path,
                        "success": false,
                        "error": format!("Failed to create parent directory: {}", e)
                    }));
                    continue;
                }
            }
            
            match tokio::fs::write(&full_path, content).await {
                Ok(_) => {
                    results.push(json!({
                        "path": path,
                        "success": true,
                        "bytes_written": content.len()
                    }));
                }
                Err(e) => {
                    results.push(json!({
                        "path": path,
                        "success": false,
                        "error": e.to_string()
                    }));
                }
            }
        }
        
        Ok(json!({
            "files": results,
            "total": files.len(),
            "successful": results.iter().filter(|r| r["success"].as_bool().unwrap_or(false)).count()
        }))
    }

    async fn execute_search_code(&self, input: &Value, project_id: &str) -> Result<Value> {
        let pattern = input.get("pattern")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'pattern' parameter"))?;
        
        let limit = input.get("limit")
            .and_then(|v| v.as_i64())
            .unwrap_or(50) as i32;
        
        info!("Searching code: pattern={} limit={}", pattern, limit);
        
        let results = self.code_intelligence
            .search_elements_for_project(pattern, project_id, Some(limit))
            .await?;
        
        Ok(json!({
            "pattern": pattern,
            "results": results,
            "count": results.len(),
        }))
    }

    async fn execute_project_context(&self, _input: &Value, project_id: &str) -> Result<Value> {
        crate::tools::project_context::get_project_context(project_id, &self.sqlite_pool).await
    }

    /// Get repository path for a project
    async fn get_repo_path(&self, project_id: &str) -> Result<PathBuf> {
        let attachment = sqlx::query!(
            r#"SELECT local_path FROM git_repo_attachments WHERE project_id = ? LIMIT 1"#,
            project_id
        )
        .fetch_optional(&self.sqlite_pool)
        .await?
        .ok_or_else(|| anyhow::anyhow!("No repository attached to project {}", project_id))?;
        
        Ok(PathBuf::from(attachment.local_path))
    }
}
