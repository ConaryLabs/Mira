// src/operations/engine/tool_router/file_routes.rs
// Complex file operation routing with native GPT-5.1 tool support

use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::sync::Arc;
use tracing::{debug, info, warn};

use crate::llm::provider::openai::{PatchOperation, PatchOpType};

use crate::llm::provider::{LlmProvider, Message};
use crate::operations::get_file_low_level_tools;
use crate::prompt::internal::tool_router as prompts;
use super::super::file_handlers::FileHandlers;
use super::llm_conversation::execute_file_read_conversation;

/// Route read_project_file to read_file tool
///
/// Supports reading multiple files in one call (optimized for token usage)
pub async fn route_read_file(
    llm: &Arc<dyn LlmProvider>,
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
    llm: &Arc<dyn LlmProvider>,
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
        .chat_with_tools(messages, system_prompt.to_string(), tools, None)
        .await
        .context("LLM search failed")?;

    // Execute grep tool if called
    if let Some(tool_call) = response.function_calls.first() {
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
    llm: &Arc<dyn LlmProvider>,
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
        .chat_with_tools(messages, system_prompt.to_string(), tools, None)
        .await
        .context("LLM list failed")?;

    // Execute list_files tool if called
    if let Some(tool_call) = response.function_calls.first() {
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

// ============================================================================
// Native GPT-5.1 Tool Handlers
// ============================================================================

/// Route native apply_patch calls from GPT-5.1
///
/// Parses V4A diff format and applies each operation:
/// - Add File: Creates new file with content
/// - Update File: Applies diff hunks to existing file
/// - Delete File: Removes file
///
/// V4A format is GPT-5.1's native patch format with 35% fewer failures
/// compared to custom edit tools.
pub async fn route_native_apply_patch(file_handlers: &FileHandlers, args: Value) -> Result<Value> {
    let patch = args
        .get("patch")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing 'patch' argument"))?;

    info!("[ROUTER] Native apply_patch: {} bytes", patch.len());

    // Parse V4A format into operations
    let operations = PatchOperation::parse_v4a(patch);

    if operations.is_empty() {
        return Ok(json!({
            "success": false,
            "error": "No valid patch operations found in V4A format"
        }));
    }

    debug!("[ROUTER] Parsed {} operations from V4A patch", operations.len());

    let mut results = Vec::new();
    let mut success_count = 0;
    let mut error_count = 0;

    for op in &operations {
        let result = match op.op_type {
            PatchOpType::Create => {
                info!("[ROUTER] Creating file: {}", op.path);
                let write_args = json!({
                    "path": op.path,
                    "content": op.content
                });
                file_handlers.execute_tool("write_file", write_args).await
            }
            PatchOpType::Update => {
                info!("[ROUTER] Updating file: {}", op.path);
                // Apply unified diff format
                apply_unified_diff(file_handlers, &op.path, &op.content).await
            }
            PatchOpType::Delete => {
                info!("[ROUTER] Deleting file: {}", op.path);
                let delete_args = json!({
                    "path": op.path
                });
                file_handlers.execute_tool("delete_file", delete_args).await
            }
        };

        match result {
            Ok(r) => {
                success_count += 1;
                results.push(json!({
                    "path": op.path,
                    "operation": format!("{:?}", op.op_type),
                    "success": true,
                    "result": r
                }));
            }
            Err(e) => {
                error_count += 1;
                warn!("[ROUTER] Patch operation failed for {}: {}", op.path, e);
                results.push(json!({
                    "path": op.path,
                    "operation": format!("{:?}", op.op_type),
                    "success": false,
                    "error": e.to_string()
                }));
            }
        }
    }

    Ok(json!({
        "success": error_count == 0,
        "operations_total": operations.len(),
        "operations_succeeded": success_count,
        "operations_failed": error_count,
        "results": results
    }))
}

/// Apply a unified diff to a file
///
/// Parses diff hunks and applies them sequentially
async fn apply_unified_diff(
    file_handlers: &FileHandlers,
    path: &str,
    diff_content: &str,
) -> Result<Value> {
    // First, read the current file content
    let read_args = json!({ "path": path });
    let file_result = file_handlers.execute_tool("read_file", read_args).await?;

    let current_content = file_result
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let mut lines: Vec<&str> = current_content.lines().collect();
    let mut offset: i64 = 0;

    // Parse and apply each hunk
    for hunk in parse_diff_hunks(diff_content) {
        let adjusted_start = (hunk.old_start as i64 + offset - 1).max(0) as usize;

        // Remove old lines
        let end_remove = (adjusted_start + hunk.old_count).min(lines.len());
        if adjusted_start < lines.len() {
            lines.drain(adjusted_start..end_remove);
        }

        // Insert new lines at the same position
        for (i, line) in hunk.new_lines.iter().enumerate() {
            let insert_pos = (adjusted_start + i).min(lines.len());
            lines.insert(insert_pos, line);
        }

        // Adjust offset for subsequent hunks
        offset += hunk.new_count as i64 - hunk.old_count as i64;
    }

    // Write the modified content
    let new_content = lines.join("\n");
    let write_args = json!({
        "path": path,
        "content": new_content
    });

    file_handlers.execute_tool("write_file", write_args).await
}

/// Parsed diff hunk
struct DiffHunk<'a> {
    old_start: usize,
    old_count: usize,
    new_count: usize,
    new_lines: Vec<&'a str>,
}

/// Parse unified diff hunks from diff content
fn parse_diff_hunks(diff: &str) -> Vec<DiffHunk<'_>> {
    let mut hunks = Vec::new();
    let mut current_hunk: Option<DiffHunk> = None;

    for line in diff.lines() {
        if line.starts_with("@@") {
            // Save previous hunk if exists
            if let Some(h) = current_hunk.take() {
                hunks.push(h);
            }

            // Parse hunk header: @@ -start,count +start,count @@
            if let Some((old_info, new_info)) = parse_hunk_header(line) {
                current_hunk = Some(DiffHunk {
                    old_start: old_info.0,
                    old_count: old_info.1,
                    new_count: new_info.1,
                    new_lines: Vec::new(),
                });
            }
        } else if let Some(ref mut hunk) = current_hunk {
            // Process diff lines
            if line.starts_with('+') && !line.starts_with("+++") {
                // Added line (strip the + prefix)
                hunk.new_lines.push(&line[1..]);
            } else if line.starts_with(' ') {
                // Context line (unchanged, include in new lines)
                hunk.new_lines.push(&line[1..]);
            }
            // Lines starting with - are removed (don't add to new_lines)
        }
    }

    // Don't forget the last hunk
    if let Some(h) = current_hunk {
        hunks.push(h);
    }

    hunks
}

/// Parse hunk header line
/// Format: @@ -old_start,old_count +new_start,new_count @@
fn parse_hunk_header(line: &str) -> Option<((usize, usize), (usize, usize))> {
    let line = line.trim_start_matches('@').trim();
    let parts: Vec<&str> = line.split_whitespace().collect();

    if parts.len() < 2 {
        return None;
    }

    let old_part = parts[0].trim_start_matches('-');
    let new_part = parts[1].trim_start_matches('+').trim_end_matches('@').trim();

    let old = parse_range(old_part)?;
    let new = parse_range(new_part)?;

    Some((old, new))
}

/// Parse range like "10,3" or "10" (implicit count of 1)
fn parse_range(s: &str) -> Option<(usize, usize)> {
    let parts: Vec<&str> = s.split(',').collect();
    let start: usize = parts.first()?.parse().ok()?;
    let count: usize = parts.get(1).and_then(|c| c.parse().ok()).unwrap_or(1);
    Some((start, count))
}
