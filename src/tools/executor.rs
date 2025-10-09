// src/tools/executor.rs
// Smart tool executor: GPT-5 calls tools, DeepSeek handles heavy lifting behind the scenes

use anyhow::Result;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::sync::Arc;
use std::path::Path;
use tracing::{info, debug, warn};

use crate::memory::features::code_intelligence::CodeIntelligenceService;
use crate::llm::router::{LlmRouter, TaskType};
use crate::llm::provider::Message;

pub struct ToolExecutor {
    code_intelligence: Arc<CodeIntelligenceService>,
    sqlite_pool: SqlitePool,
    llm_router: Arc<LlmRouter>,  // NEW: For DeepSeek delegation
}

impl ToolExecutor {
    pub fn new(
        code_intelligence: Arc<CodeIntelligenceService>,
        sqlite_pool: SqlitePool,
        llm_router: Arc<LlmRouter>,
    ) -> Self {
        Self {
            code_intelligence,
            sqlite_pool,
            llm_router,
        }
    }

    /// Execute a tool by name
    /// Automatically delegates heavy operations to DeepSeek
    pub async fn execute_tool(&self, tool_name: &str, input: &Value, project_id: &str) -> Result<Value> {
        match tool_name {
            // Light tools - execute normally
            "create_artifact" => self.execute_create_artifact(input).await,
            "read_file" => self.execute_read_file(input, project_id).await,
            "list_files" => self.execute_list_files(input, project_id).await,
            "read_files" => self.execute_read_files(input, project_id).await,
            "write_files" => self.execute_write_files(input, project_id).await,
            
            // Heavy tools - delegate to DeepSeek internally
            "provide_code_fix" => self.execute_code_fix_with_deepseek(input, project_id).await,
            "search_code" => self.execute_search_with_deepseek(input, project_id).await,
            "get_project_context" => self.execute_project_context_with_deepseek(input, project_id).await,
            
            _ => Err(anyhow::anyhow!("Unknown tool: {}", tool_name)),
        }
    }

    // ===== LIGHT TOOLS (Execute Normally) =====

    /// Create a code artifact with syntax highlighting
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

    async fn try_list_project_repo(&self, path: &str, project_id: &str) -> Result<Option<Vec<Value>>> {
        let attachment = match sqlx::query!(
            r#"SELECT local_path FROM git_repo_attachments WHERE project_id = ? LIMIT 1"#,
            project_id
        )
        .fetch_optional(&self.sqlite_pool)
        .await? {
            Some(att) => att,
            None => {
                debug!("No git attachment for project {}", project_id);
                return Ok(None);
            }
        };

        let base_path = Path::new(&attachment.local_path).join(path);
        
        let mut dir = match tokio::fs::read_dir(&base_path).await {
            Ok(d) => d,
            Err(e) => {
                debug!("Failed to list project repo directory: {}", e);
                return Ok(None);
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

    async fn execute_write_files(&self, input: &Value, project_id: &str) -> Result<Value> {
        let files = input.get("files")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow::anyhow!("Missing 'files' array parameter"))?;

        info!("Writing {} files in batch", files.len());
        
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

    // ===== HEAVY TOOLS (Delegate to DeepSeek) =====

    /// Generate code fix using DeepSeek (heavy token operation)
    async fn execute_code_fix_with_deepseek(&self, input: &Value, project_id: &str) -> Result<Value> {
        info!("🔧 Delegating code fix generation to DeepSeek");
        
        // Extract fix request details
        let error_message = input.get("error_message")
            .and_then(|v| v.as_str())
            .unwrap_or("Fix the error in this code");
        
        let file_path = input.get("file_path")
            .and_then(|v| v.as_str());
        
        let error_context = input.get("error_context")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        
        // Load the file if path provided
        let file_content = if let Some(path) = file_path {
            match super::file_ops::load_complete_file(&self.sqlite_pool, path, project_id).await {
                Ok(content) => Some(content),
                Err(e) => {
                    warn!("Failed to load file for code fix: {}", e);
                    None
                }
            }
        } else {
            None
        };
        
        // Build comprehensive prompt for DeepSeek
        let prompt = format!(
            r#"You are fixing a code error. Provide a COMPLETE fixed version of the file.

Error: {}

Error Context:
{}

{}

Requirements:
- Provide the COMPLETE fixed file from line 1 to the end
- Include ALL imports, ALL functions, ALL code
- Fix the specific error mentioned
- Do NOT use ellipsis (...) or placeholders
- Do NOT truncate any code
- Return only the fixed code, no explanations before or after

Return the complete fixed file:"#,
            error_message,
            error_context,
            if let Some(content) = &file_content {
                format!("Current File Content:\n```\n{}\n```", content)
            } else {
                "No file content provided.".to_string()
            }
        );
        
        // Call DeepSeek to generate the fix
        let response = self.llm_router.chat(
            TaskType::Code,
            vec![Message {
                role: "user".to_string(),
                content: prompt,
            }],
            "You are a code fixing assistant. Return complete fixed files with no truncation.".to_string(),
        ).await?;
        
        info!(
            "✅ DeepSeek generated fix | Tokens: in={} out={} cached={} | {}ms",
            response.tokens.input,
            response.tokens.output,
            response.tokens.cached,
            response.latency_ms
        );
        
        // Extract code from response (handle code blocks)
        let fixed_content = if response.content.contains("```") {
            // Extract from code block
            response.content
                .split("```")
                .nth(1)
                .map(|block| {
                    // Skip language identifier line
                    block.lines().skip(1).collect::<Vec<_>>().join("\n")
                })
                .unwrap_or(response.content.clone())
        } else {
            response.content.clone()
        };
        
        // Format as artifact
        let language = file_path
            .and_then(|p| Path::new(p).extension())
            .and_then(|ext| ext.to_str())
            .and_then(|ext| match ext {
                "rs" => Some("rust"),
                "ts" | "tsx" => Some("typescript"),
                "js" | "jsx" => Some("javascript"),
                "py" => Some("python"),
                "go" => Some("go"),
                "java" => Some("java"),
                _ => Some("text"),
            })
            .unwrap_or("text");
        
        Ok(json!({
            "type": "code_fix",
            "artifacts": [{
                "title": file_path.unwrap_or("fixed_code"),
                "content": fixed_content.trim(),
                "language": language,
                "path": file_path,
                "lines": fixed_content.lines().count(),
                "is_fix": true,
            }],
            "message": "Generated code fix",
            "deepseek_tokens": {
                "input": response.tokens.input,
                "output": response.tokens.output,
                "cached": response.tokens.cached,
            }
        }))
    }

    /// Search code using DeepSeek for large codebases
    async fn execute_search_with_deepseek(&self, input: &Value, project_id: &str) -> Result<Value> {
        let query = input.get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'query' parameter"))?;

        info!("Searching code: {}", query);
        
        // Get raw search results from code intelligence
        let raw_results = self.code_intelligence
            .search_elements_for_project(query, project_id, Some(50))
            .await?;
        
        // If results are small, return directly (no DeepSeek needed)
        if raw_results.len() <= 10 {
            return Ok(json!({
                "query": query,
                "results": raw_results,
                "count": raw_results.len(),
                "source": "direct"
            }));
        }
        
        info!("🔧 Large result set ({}), using DeepSeek to summarize", raw_results.len());
        
        // Use DeepSeek to analyze and summarize large result sets
        let results_summary = format!(
            "Found {} code elements matching '{}'. Summarize the most relevant ones:\n\n{}",
            raw_results.len(),
            query,
            serde_json::to_string_pretty(&raw_results)?
        );
        
        let response = self.llm_router.chat(
            TaskType::Code,
            vec![Message {
                role: "user".to_string(),
                content: results_summary,
            }],
            "You are a code search assistant. Summarize search results concisely, focusing on the most relevant findings.".to_string(),
        ).await?;
        
        info!(
            "✅ DeepSeek summarized search | Tokens: in={} out={}",
            response.tokens.input,
            response.tokens.output
        );
        
        Ok(json!({
            "query": query,
            "summary": response.content,
            "total_found": raw_results.len(),
            "raw_results": raw_results.iter().take(20).collect::<Vec<_>>(),  // Include top 20 raw
            "source": "deepseek_summarized"
        }))
    }

    /// Get project context using DeepSeek to analyze structure
    async fn execute_project_context_with_deepseek(&self, _input: &Value, project_id: &str) -> Result<Value> {
        info!("🔧 Delegating project analysis to DeepSeek");
        
        // Get basic context first
        let basic_context = crate::tools::project_context::get_project_context(
            project_id, 
            &self.sqlite_pool
        ).await?;
        
        // Check if project is large enough to need DeepSeek analysis
        let file_count = basic_context.get("file_count")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        
        // Small projects don't need DeepSeek
        if file_count < 50 {
            return Ok(basic_context);
        }
        
        info!("Large project ({} files), using DeepSeek for analysis", file_count);
        
        // Use DeepSeek to analyze and summarize project structure
        let analysis_prompt = format!(
            r#"Analyze this project structure and provide a concise summary:

{}

Provide:
1. Project type and main language
2. Key directories and their purposes
3. Main entry points
4. Notable patterns or architecture
5. Potential areas of concern

Be concise and focus on what's most relevant for understanding the codebase."#,
            serde_json::to_string_pretty(&basic_context)?
        );
        
        let response = self.llm_router.chat(
            TaskType::Code,
            vec![Message {
                role: "user".to_string(),
                content: analysis_prompt,
            }],
            "You are a code architecture analyst. Provide clear, actionable insights about codebases.".to_string(),
        ).await?;
        
        info!(
            "✅ DeepSeek analyzed project | Tokens: in={} out={}",
            response.tokens.input,
            response.tokens.output
        );
        
        // Combine basic context with DeepSeek analysis
        let mut result = basic_context;
        result["analysis"] = json!(response.content);
        result["analyzed_by"] = json!("deepseek");
        
        Ok(result)
    }
}
