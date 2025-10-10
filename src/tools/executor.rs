// src/tools/executor.rs
// Tool executor providing access to file operations, code search, and project context

use anyhow::Result;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::sync::Arc;
use tracing::{info, warn};
use sha2::{Sha256, Digest};

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
            "lines": content.lines().count(),
        }))
    }

    async fn execute_list_files(&self, input: &Value, project_id: &str) -> Result<Value> {
        let path = input.get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        
        let files = super::file_ops::list_project_files(&self.sqlite_pool, path, project_id).await?;
        
        Ok(json!({
            "path": path,
            "files": files,
            "count": files.len(),
        }))
    }

    async fn execute_read_files(&self, input: &Value, project_id: &str) -> Result<Value> {
        let paths = input.get("paths")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow::anyhow!("Missing 'paths' array"))?;
        
        let mut results = Vec::new();
        for path in paths {
            let path_str = path.as_str().ok_or_else(|| anyhow::anyhow!("Invalid path"))?;
            match super::file_ops::load_complete_file(&self.sqlite_pool, path_str, project_id).await {
                Ok(content) => {
                    results.push(json!({
                        "path": path_str,
                        "content": content,
                        "success": true
                    }));
                }
                Err(e) => {
                    results.push(json!({
                        "path": path_str,
                        "error": e.to_string(),
                        "success": false
                    }));
                }
            }
        }
        
        Ok(json!({
            "files": results,
            "count": results.len()
        }))
    }

    async fn execute_write_files(&self, input: &Value, project_id: &str) -> Result<Value> {
        let files = input.get("files")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow::anyhow!("Missing 'files' array"))?;
        
        let mut results = Vec::new();
        let mut files_to_parse: Vec<(String, String)> = Vec::new();
        
        for file in files {
            let path = file.get("path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing file path"))?;
            
            let content = file.get("content")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing file content"))?;
            
            match crate::file_system::write_file_with_history(
                &self.sqlite_pool,
                project_id,
                path,
                content,
            ).await {
                Ok(_) => {
                    results.push(json!({
                        "path": path,
                        "success": true,
                        "bytes_written": content.len()
                    }));
                    
                    // Collect files to parse (Layer 1: Auto-parse after writes)
                    if should_parse_file(path) {
                        files_to_parse.push((path.to_string(), content.to_string()));
                    }
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
        
        // Parse all parseable files that were successfully written
        for (file_path, content) in files_to_parse {
            match self.parse_and_store_file(project_id, &file_path, &content).await {
                Ok(_) => info!("Auto-parsed after write: {}", file_path),
                Err(e) => warn!("Parse failed (non-fatal): {} - {}", file_path, e),
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

    /// Parse and store a file's AST (Layer 1: Auto-parse after writes)
    async fn parse_and_store_file(&self, project_id: &str, file_path: &str, content: &str) -> Result<()> {
        // Get or create file record
        let file_id = self.upsert_repository_file(project_id, file_path, content).await?;
        
        // Detect language
        let language = detect_language_from_path(file_path);
        
        // Parse and store code elements
        self.code_intelligence
            .analyze_and_store_with_project(file_id, file_path, content, project_id, &language)
            .await?;
        
        Ok(())
    }

    /// Get or create repository_files record
    async fn upsert_repository_file(&self, project_id: &str, file_path: &str, content: &str) -> Result<i64> {
        // Get attachment_id for this project
        let attachment = sqlx::query!(
            r#"
            SELECT id FROM git_repo_attachments
            WHERE project_id = ?
            LIMIT 1
            "#,
            project_id
        )
        .fetch_optional(&self.sqlite_pool)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Project has no attachment (git repo or local directory)"))?;
        
        let attachment_id = attachment.id;
        
        // Calculate content hash
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        let hash = format!("{:x}", hasher.finalize());
        
        // Detect language
        let language = detect_language_from_path(file_path);
        
        // Insert or update file record
        let file_id = sqlx::query_scalar!(
            r#"
            INSERT INTO repository_files (attachment_id, file_path, content_hash, language, last_indexed)
            VALUES (?, ?, ?, ?, strftime('%s','now'))
            ON CONFLICT(attachment_id, file_path) DO UPDATE SET
                content_hash = excluded.content_hash,
                language = excluded.language,
                last_indexed = strftime('%s','now')
            RETURNING id
            "#,
            attachment_id,
            file_path,
            hash,
            language
        )
        .fetch_one(&self.sqlite_pool)
        .await?;
        
        Ok(file_id)
    }
}

/// Check if file should be parsed for code intelligence
fn should_parse_file(path: &str) -> bool {
    let parseable_extensions = [
        ".rs",   // Rust
        ".ts",   // TypeScript
        ".tsx",  // TypeScript React
        ".js",   // JavaScript
        ".jsx",  // JavaScript React
    ];
    
    parseable_extensions.iter().any(|ext| path.ends_with(ext))
}

/// Detect programming language from file extension
fn detect_language_from_path(path: &str) -> String {
    if path.ends_with(".rs") {
        "rust".to_string()
    } else if path.ends_with(".ts") || path.ends_with(".tsx") {
        "typescript".to_string()
    } else if path.ends_with(".js") || path.ends_with(".jsx") {
        "javascript".to_string()
    } else {
        "unknown".to_string()
    }
}
