// src/tools/implementations.rs
// Concrete tool implementations that hook into the existing ToolExecutor

use anyhow::{Result, anyhow};
use serde_json::{json, Value};
use tracing::{info, debug, warn};
use std::sync::Arc;

use crate::state::AppState;
use crate::tools::executor::ToolExecutor;

/// Web search tool implementation
pub async fn execute_web_search(
    query: &str,
    _app_state: &Arc<AppState>,
) -> Result<Value> {
    info!("Executing web search for query: '{}'", query);
    
    // TODO: Implement actual web search when API keys are available
    // For now, return a mock response
    warn!("Web search not fully implemented - returning mock results");
    
    Ok(json!({
        "tool_type": "web_search",
        "query": query,
        "results": [
            {
                "title": "Mock Result 1",
                "url": "https://example.com/1",
                "snippet": "This is a mock search result for testing"
            },
            {
                "title": "Mock Result 2", 
                "url": "https://example.com/2",
                "snippet": "Another mock result demonstrating the structure"
            }
        ],
        "status": "mock_implementation",
        "message": "Web search will be functional when API keys are configured"
    }))
}

/// Code interpreter tool implementation
pub async fn execute_code_interpreter(
    code: &str,
    language: &str,
    _app_state: &Arc<AppState>,
) -> Result<Value> {
    info!("Executing code interpreter for {} code", language);
    debug!("Code to execute: {}", code);
    
    // TODO: Implement actual sandboxed code execution
    // This would require Docker or similar containerization
    warn!("Code interpreter not fully implemented - returning mock output");
    
    // For now, do some basic validation
    let is_safe = !code.contains("import os") && 
                  !code.contains("import subprocess") &&
                  !code.contains("eval(") &&
                  !code.contains("exec(");
    
    if !is_safe {
        return Ok(json!({
            "tool_type": "code_interpreter",
            "language": language,
            "status": "blocked",
            "error": "Code contains potentially unsafe operations",
            "message": "For security, certain operations are not allowed"
        }));
    }
    
    Ok(json!({
        "tool_type": "code_interpreter",
        "language": language,
        "code": code,
        "output": format!("Mock output for {} code execution", language),
        "status": "mock_implementation",
        "message": "Code interpreter will be functional when sandbox is configured"
    }))
}

/// Load file context from repository
pub async fn execute_load_file_context(
    project_id: &str,
    file_paths: Option<Vec<String>>,
    app_state: &Arc<AppState>,
) -> Result<Value> {
    info!("Loading file context for project: {}", project_id);
    
    // Get git attachments for the project
    let attachments = app_state.git_store
        .get_attachments_for_project(project_id)
        .await?;
    
    if attachments.is_empty() {
        return Ok(json!({
            "tool_type": "load_file_context",
            "project_id": project_id,
            "status": "no_repository",
            "message": "No repository attached to this project"
        }));
    }
    
    let attachment = &attachments[0];
    let repo_path = std::path::Path::new(&attachment.local_path);
    
    if !repo_path.exists() {
        return Ok(json!({
            "tool_type": "load_file_context",
            "project_id": project_id,
            "status": "repository_not_found",
            "message": "Repository not cloned locally"
        }));
    }
    
    let mut files_content = Vec::new();
    
    if let Some(paths) = file_paths {
        // Load specific files
        for path in paths {
            let file_path = repo_path.join(&path);
            if file_path.exists() {
                if let Ok(content) = std::fs::read_to_string(&file_path) {
                    files_content.push(json!({
                        "path": path,
                        "content": content,
                        "size": content.len()
                    }));
                }
            }
        }
    } else {
        // Load all text files (with size limit)
        let max_total_size = 500_000; // 500KB total
        let max_file_size = 100_000;  // 100KB per file
        let mut total_size = 0;
        
        for entry in walkdir::WalkDir::new(repo_path)
            .max_depth(5)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            
            // Skip non-files and binary files
            if !path.is_file() || is_binary_file(path) {
                continue;
            }
            
            // Skip large files
            if let Ok(metadata) = path.metadata() {
                if metadata.len() > max_file_size as u64 {
                    continue;
                }
            }
            
            // Read file content
            if let Ok(content) = std::fs::read_to_string(path) {
                let size = content.len();
                if total_size + size > max_total_size {
                    break;
                }
                
                let relative_path = path.strip_prefix(repo_path)
                    .unwrap_or(path)
                    .to_string_lossy();
                
                files_content.push(json!({
                    "path": relative_path,
                    "content": content,
                    "size": size
                }));
                
                total_size += size;
            }
        }
    }
    
    Ok(json!({
        "tool_type": "load_file_context",
        "project_id": project_id,
        "repository": attachment.repo_url,
        "files": files_content,
        "file_count": files_content.len(),
        "status": "success"
    }))
}

/// Check if a file is likely binary
fn is_binary_file(path: &std::path::Path) -> bool {
    let binary_extensions = vec![
        "exe", "dll", "so", "dylib", "jar", "class",
        "png", "jpg", "jpeg", "gif", "bmp", "ico", "svg",
        "mp3", "mp4", "avi", "mov", "wmv",
        "zip", "tar", "gz", "rar", "7z",
        "pdf", "doc", "docx", "xls", "xlsx",
        "pyc", "pyo", "o", "a", "lib"
    ];
    
    if let Some(ext) = path.extension() {
        let ext_str = ext.to_string_lossy().to_lowercase();
        return binary_extensions.contains(&ext_str.as_str());
    }
    
    false
}

/// Extension trait to add tool execution methods to ToolExecutor
pub trait ToolExecutorExt {
    async fn handle_tool_call(&self, tool_call: &Value, app_state: &Arc<AppState>) -> Result<Value>;
}

impl ToolExecutorExt for ToolExecutor {
    async fn handle_tool_call(&self, tool_call: &Value, app_state: &Arc<AppState>) -> Result<Value> {
        let tool_type = tool_call["type"].as_str()
            .or_else(|| tool_call["function"]["name"].as_str())
            .ok_or_else(|| anyhow!("Missing tool type"))?;
        
        let args = tool_call.get("arguments")
            .or_else(|| tool_call.get("function").and_then(|f| f.get("arguments")))
            .ok_or_else(|| anyhow!("Missing tool arguments"))?;
        
        // Parse arguments if they're a string
        let parsed_args: Value = if args.is_string() {
            serde_json::from_str(args.as_str().unwrap())?
        } else {
            args.clone()
        };
        
        match tool_type {
            "file_search" => {
                let query = parsed_args["query"].as_str()
                    .ok_or_else(|| anyhow!("Missing query for file search"))?;
                let project_id = parsed_args["project_id"].as_str();
                let file_extensions = parsed_args["file_extensions"]
                    .as_array()
                    .map(|arr| arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect());
                let max_files = parsed_args["max_files"].as_u64().map(|n| n as usize);
                let case_sensitive = parsed_args["case_sensitive"].as_bool();
                
                self.execute_file_search(query, project_id, file_extensions, max_files, case_sensitive).await
            }
            
            "image_generation" | "generate_image" => {
                let prompt = parsed_args["prompt"].as_str()
                    .ok_or_else(|| anyhow!("Missing prompt for image generation"))?;
                let style = parsed_args["style"].as_str().map(String::from);
                let quality = parsed_args["quality"].as_str().map(String::from);
                let size = parsed_args["size"].as_str().map(String::from);
                
                self.execute_image_generation(prompt, style, quality, size).await
            }
            
            "web_search" => {
                let query = parsed_args["query"].as_str()
                    .ok_or_else(|| anyhow!("Missing query for web search"))?;
                execute_web_search(query, app_state).await
            }
            
            "code_interpreter" => {
                let code = parsed_args["code"].as_str()
                    .ok_or_else(|| anyhow!("Missing code for interpreter"))?;
                let language = parsed_args["language"].as_str().unwrap_or("python");
                execute_code_interpreter(code, language, app_state).await
            }
            
            "load_file_context" => {
                let project_id = parsed_args["project_id"].as_str()
                    .ok_or_else(|| anyhow!("Missing project_id for file context"))?;
                let file_paths = parsed_args["file_paths"]
                    .as_array()
                    .map(|arr| arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect());
                execute_load_file_context(project_id, file_paths, app_state).await
            }
            
            _ => {
                warn!("Unknown tool type: {}", tool_type);
                Ok(json!({
                    "tool_type": tool_type,
                    "status": "unknown_tool",
                    "error": format!("Tool '{}' is not implemented", tool_type)
                }))
            }
        }
    }
}
