// src/operations/engine/tool_router/file_routes.rs
// Complex file operation routing

use anyhow::{Context, Result};
use serde_json::{json, Value};
use tracing::{info, warn};

use crate::llm::provider::{Gemini3Provider, Message};
use crate::operations::get_file_low_level_tools;
use crate::prompt::internal::tool_router as prompts;
use super::super::file_handlers::FileHandlers;
use super::llm_conversation::execute_file_read_conversation;

/// Route read_project_file to read_file tool
///
/// Supports reading multiple files in one call (optimized for token usage)
pub async fn route_read_file(
    llm: &Gemini3Provider,
    file_handlers: &FileHandlers,
    args: Value,
) -> Result<Value> {
    let paths = args
        .get("paths")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow::anyhow!("Missing 'paths' array"))?;

    let purpose = args
        .get("purpose")
        .and_then(|v| v.as_str())
        .unwrap_or("Reading project files");

    info!(
        "[ROUTER] Reading {} file(s) for: {}",
        paths.len(),
        purpose
    );

    // Build prompt with file reading tools
    let system_prompt = format!(
        "{} Purpose: {}",
        prompts::FILE_READER, purpose
    );

    let user_prompt = format!(
        "Please read the following files:\n{}\n\n\
        For each file, use the read_file tool.",
        paths
            .iter()
            .filter_map(|p| p.as_str())
            .collect::<Vec<_>>()
            .join("\n- ")
    );

    let messages = vec![
        Message::system(system_prompt),
        Message::user(user_prompt),
    ];

    let tools = get_file_low_level_tools();

    let result = execute_file_read_conversation(llm, file_handlers, messages, tools).await?;

    // Return aggregated results to LLM
    Ok(json!({
        "success": true,
        "files_read": result.files.len(),
        "files": result.files,
        "summary": result.summary,
        "tokens_used": {
            "input": result.tokens_input,
            "output": result.tokens_output
        }
    }))
}

/// Route search_codebase to grep_files tool
pub async fn route_search(
    llm: &Gemini3Provider,
    file_handlers: &FileHandlers,
    args: Value,
) -> Result<Value> {
    let query = args
        .get("query")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing 'query' parameter"))?;

    let file_pattern = args.get("file_pattern").and_then(|v| v.as_str());
    let case_sensitive = args
        .get("case_sensitive")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    info!("[ROUTER] Searching codebase for: '{}'", query);

    // Build prompt for searching
    let system_prompt = prompts::CODE_SEARCHER;

    let mut user_prompt = format!("Search for: {}", query);
    if let Some(pattern) = file_pattern {
        user_prompt.push_str(&format!("\nLimit to files matching: {}", pattern));
    }

    let messages = vec![
        Message::system(system_prompt.to_string()),
        Message::user(user_prompt),
    ];

    let tools = get_file_low_level_tools();
    let response = llm
        .call_with_tools(messages, tools)
        .await
        .context("LLM search failed")?;

    // Execute grep tool if called
    if let Some(tool_call) = response.tool_calls.first() {
        if tool_call.name == "grep_files" {
            return file_handlers
                .execute_tool(&tool_call.name, tool_call.arguments.clone())
                .await;
        }
    }

    // Fallback: execute grep directly
    let grep_args = json!({
        "pattern": query,
        "file_pattern": file_pattern,
        "case_insensitive": !case_sensitive
    });

    file_handlers.execute_tool("grep_files", grep_args).await
}

/// Route list_project_files to list_files tool
pub async fn route_list_files(
    llm: &Gemini3Provider,
    file_handlers: &FileHandlers,
    args: Value,
) -> Result<Value> {
    let directory = args
        .get("directory")
        .and_then(|v| v.as_str())
        .unwrap_or(".");
    let pattern = args.get("pattern").and_then(|v| v.as_str());
    let recursive = args
        .get("recursive")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    info!("[ROUTER] Listing files in: {}", directory);

    // Build prompt for listing
    let system_prompt = prompts::FILE_LISTER;

    let mut user_prompt = format!("List files in directory: {}", directory);
    if let Some(p) = pattern {
        user_prompt.push_str(&format!("\nFilter by pattern: {}", p));
    }
    if recursive {
        user_prompt.push_str("\nInclude subdirectories recursively");
    }

    let messages = vec![
        Message::system(system_prompt.to_string()),
        Message::user(user_prompt),
    ];

    let tools = get_file_low_level_tools();
    let response = llm
        .call_with_tools(messages, tools)
        .await
        .context("LLM list failed")?;

    // Execute list_files tool if called
    if let Some(tool_call) = response.tool_calls.first() {
        if tool_call.name == "list_files" {
            return file_handlers
                .execute_tool(&tool_call.name, tool_call.arguments.clone())
                .await;
        }
    }

    // Fallback: execute list_files directly
    let list_args = json!({
        "directory": directory,
        "pattern": pattern,
        "recursive": recursive
    });

    file_handlers.execute_tool("list_files", list_args).await
}

/// Route get_file_summary to summarize_file tool
///
/// Token-optimized: Returns only preview + stats instead of full content
pub async fn route_file_summary(file_handlers: &FileHandlers, args: Value) -> Result<Value> {
    let paths = args
        .get("paths")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow::anyhow!("Missing 'paths' array"))?;

    let preview_lines = args
        .get("preview_lines")
        .and_then(|v| v.as_str())
        .unwrap_or("10");

    info!("[ROUTER] Summarizing {} file(s)", paths.len());

    let mut summaries = Vec::new();

    for path_val in paths {
        if let Some(path) = path_val.as_str() {
            let summary_args = json!({
                "path": path,
                "preview_lines": preview_lines
            });

            match file_handlers.execute_tool("summarize_file", summary_args).await {
                Ok(result) => summaries.push(result),
                Err(e) => {
                    warn!("[ROUTER] Failed to summarize {}: {}", path, e);
                    summaries.push(json!({
                        "success": false,
                        "path": path,
                        "error": e.to_string()
                    }));
                }
            }
        }
    }

    Ok(json!({
        "success": true,
        "file_count": summaries.len(),
        "summaries": summaries
    }))
}

/// Route get_file_structure to extract_symbols tool
///
/// Token-optimized: Returns only symbol list instead of full code
pub async fn route_file_structure(file_handlers: &FileHandlers, args: Value) -> Result<Value> {
    let paths = args
        .get("paths")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow::anyhow!("Missing 'paths' array"))?;

    info!("[ROUTER] Extracting structure from {} file(s)", paths.len());

    let mut structures = Vec::new();

    for path_val in paths {
        if let Some(path) = path_val.as_str() {
            let extract_args = json!({
                "path": path
            });

            match file_handlers.execute_tool("extract_symbols", extract_args).await {
                Ok(result) => structures.push(result),
                Err(e) => {
                    warn!("[ROUTER] Failed to extract symbols from {}: {}", path, e);
                    structures.push(json!({
                        "success": false,
                        "path": path,
                        "error": e.to_string()
                    }));
                }
            }
        }
    }

    Ok(json!({
        "success": true,
        "file_count": structures.len(),
        "structures": structures
    }))
}

/// Route write_project_file directly to file handler
pub async fn route_write_file(file_handlers: &FileHandlers, args: Value) -> Result<Value> {
    let path = args
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing 'path' argument"))?;

    let content = args
        .get("content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing 'content' argument"))?;

    info!("[ROUTER] Writing file: {}", path);

    let write_args = json!({
        "path": path,
        "content": content
    });

    file_handlers.execute_tool("write_file", write_args).await
}

/// Route write_file (unrestricted) directly to file handler
pub async fn route_write_file_unrestricted(file_handlers: &FileHandlers, args: Value) -> Result<Value> {
    let path = args
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing 'path' argument"))?;

    let content = args
        .get("content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing 'content' argument"))?;

    info!("[ROUTER] Writing unrestricted file: {}", path);

    let write_args = json!({
        "path": path,
        "content": content,
        "unrestricted": true
    });

    file_handlers.execute_tool("write_file", write_args).await
}

/// Route edit_project_file directly to file handler
pub async fn route_edit_file(file_handlers: &FileHandlers, args: Value) -> Result<Value> {
    let path = args
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing 'path' argument"))?;

    let search = args
        .get("search")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing 'search' argument"))?;

    let replace = args
        .get("replace")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing 'replace' argument"))?;

    info!("[ROUTER] Editing file: {} (search/replace)", path);

    let edit_args = json!({
        "path": path,
        "search": search,
        "replace": replace
    });

    file_handlers.execute_tool("edit_file", edit_args).await
}
