// src/tools/executor.rs
// PHASE 3 UPDATE: Added efficiency tools (get_project_context, read_files, write_files)
// ARTIFACT UPDATE: Added create_artifact and provide_code_fix execution

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
    /// PHASE 3: Added get_project_context, read_files, write_files
    /// ARTIFACT UPDATE: Added create_artifact, provide_code_fix
    pub async fn execute_tool(&self, tool_name: &str, input: &Value, project_id: &str) -> Result<Value> {
        match tool_name {
            // Artifact tools
            "create_artifact" => self.execute_create_artifact(input).await,
            "provide_code_fix" => self.execute_provide_code_fix(input).await,
            
            // Existing tools
            "read_file" => self.execute_read_file(input, project_id).await,
            "search_code" => self.execute_search_code(input, project_id).await,
            "list_files" => self.execute_list_files(input, project_id).await,
            
            // Phase 3: Efficiency tools
            "get_project_context" => self.execute_project_context(input, project_id).await,
            "read_files" => self.execute_read_files(input, project_id).await,
            "write_files" => self.execute_write_files(input, project_id).await,
            
            _ => Err(anyhow::anyhow!("Unknown tool: {}", tool_name)),
        }
    }

    // ===== ARTIFACT TOOLS =====

    /// Create a code artifact with syntax highlighting
    /// Returns artifact data for Monaco editor display
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
        
        // Return artifact in format expected by frontend
        Ok(json!({
            "type": "artifact",
            "artifact": {
                "title": title,
                "content": content,
                "language": language,
                "path": path,
                "lines": content.lines().count(),
            },
            "message": format!("Created artifact: {}", title)
        }))
    }

    /// Provide complete code fix for errors
    /// Returns artifact with fixed file content
    async fn execute_provide_code_fix(&self, input: &Value) -> Result<Value> {
        let output = input.get("output")
            .and_then(|v| v.as_str())
            .unwrap_or("Code fix provided");
        
        let files = input.get("files")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow::anyhow!("Missing 'files' array in code fix"))?;
        
        info!("Providing code fix with {} file(s)", files.len());
        
        // Convert files to artifact format
        let mut artifacts = Vec::new();
        for file in files {
            let path = file.get("path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'path' in file"))?;
            
            let content = file.get("content")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'content' in file"))?;
            
            let language = file.get("language")
                .and_then(|v| v.as_str())
                .unwrap_or_else(|| {
                    // Infer language from file extension
                    Path::new(path)
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .and_then(|ext| match ext {
                            "rs" => Some("rust"),
                            "ts" => Some("typescript"),
                            "tsx" => Some("typescript"),
                            "js" => Some("javascript"),
                            "jsx" => Some("javascript"),
                            "py" => Some("python"),
                            "go" => Some("go"),
                            "java" => Some("java"),
                            "cpp" | "cc" | "cxx" => Some("cpp"),
                            "c" => Some("c"),
                            "html" => Some("html"),
                            "css" => Some("css"),
                            "json" => Some("json"),
                            "yaml" | "yml" => Some("yaml"),
                            "sql" => Some("sql"),
                            "sh" | "bash" => Some("bash"),
                            "md" => Some("markdown"),
                            _ => Some("text"),
                        })
                        .unwrap_or("text")
                });
            
            artifacts.push(json!({
                "title": path,
                "content": content,
                "language": language,
                "path": path,
                "lines": content.lines().count(),
                "is_fix": true,
            }));
        }
        
        // Return with analysis metadata if present
        let analysis = input.get("analysis");
        
        Ok(json!({
            "type": "code_fix",
            "artifacts": artifacts,
            "message": output,
            "analysis": analysis,
            "file_count": files.len()
        }))
    }

    // ===== EXISTING TOOLS =====

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

    // ===== PHASE 3: EFFICIENCY TOOLS =====

    /// Get complete project overview in one call
    /// PHASE 3.1: Returns file tree, recent files, languages, code stats
    async fn execute_project_context(&self, _input: &Value, project_id: &str) -> Result<Value> {
        info!("Getting complete project context for: {}", project_id);
        
        // Delegate to dedicated module
        crate::tools::project_context::get_project_context(project_id, &self.sqlite_pool).await
    }

    /// Read multiple files in one batch
    /// PHASE 3.2: Reduces N file reads to 1 tool call
    async fn execute_read_files(&self, input: &Value, project_id: &str) -> Result<Value> {
        let paths = input.get("paths")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow::anyhow!("Missing 'paths' array parameter"))?;

        info!("Reading {} files in batch", paths.len());
        
        let mut results = Vec::new();
        
        for path_value in paths {
            let path = path_value.as_str()
                .ok_or_else(|| anyhow::anyhow!("Invalid path in array"))?;
            
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

    /// Write multiple files in one batch
    /// PHASE 3.2: Reduces N file writes to 1 tool call
    async fn execute_write_files(&self, input: &Value, project_id: &str) -> Result<Value> {
        let files = input.get("files")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow::anyhow!("Missing 'files' array parameter"))?;

        info!("Writing {} files in batch", files.len());
        
        // Get git attachment for base path
        let attachment = sqlx::query!(
            r#"SELECT local_path FROM git_repo_attachments WHERE project_id = ? LIMIT 1"#,
            project_id
        )
        .fetch_one(&self.sqlite_pool)
        .await?;
        
        let repo_path = Path::new(&attachment.local_path);
        let mut results = Vec::new();
        
        for file_value in files {
            let file = file_value.as_object()
                .ok_or_else(|| anyhow::anyhow!("Invalid file object in array"))?;
            
            let path = file.get("path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'path' in file object"))?;
            
            let content = file.get("content")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'content' in file object"))?;
            
            let full_path = repo_path.join(path);
            
            // Create parent directories if needed
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
}
