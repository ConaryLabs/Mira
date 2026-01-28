// crates/mira-server/src/tools/core/experts/tools.rs
// Tool definitions and execution for expert sub-agents

use super::ToolContext;
use crate::db::{recall_semantic_sync, search_memories_sync};
use crate::indexer;
use crate::llm::{Tool, ToolCall};
use crate::search::{embedding_to_bytes, find_callees, find_callers, hybrid_search};
use serde_json::{Value, json};
use std::path::Path;

/// Define the tools available to experts
pub fn get_expert_tools() -> Vec<Tool> {
    vec![
        Tool::function(
            "search_code",
            "Search for code by meaning. Use this to find relevant code snippets, functions, or patterns.",
            json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Natural language description of what you're looking for (e.g., 'authentication middleware', 'error handling in API routes')"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results (default: 5)",
                        "default": 5
                    }
                },
                "required": ["query"]
            }),
        ),
        Tool::function(
            "get_symbols",
            "Get the structure of a file - lists all functions, structs, classes, etc.",
            json!({
                "type": "object",
                "properties": {
                    "file_path": {
                        "type": "string",
                        "description": "Path to the file (relative to project root)"
                    }
                },
                "required": ["file_path"]
            }),
        ),
        Tool::function(
            "read_file",
            "Read the contents of a specific file or a range of lines.",
            json!({
                "type": "object",
                "properties": {
                    "file_path": {
                        "type": "string",
                        "description": "Path to the file (relative to project root)"
                    },
                    "start_line": {
                        "type": "integer",
                        "description": "Starting line number (1-indexed, optional)"
                    },
                    "end_line": {
                        "type": "integer",
                        "description": "Ending line number (inclusive, optional)"
                    }
                },
                "required": ["file_path"]
            }),
        ),
        Tool::function(
            "find_callers",
            "Find all functions that call a given function.",
            json!({
                "type": "object",
                "properties": {
                    "function_name": {
                        "type": "string",
                        "description": "Name of the function to find callers for"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results (default: 10)",
                        "default": 10
                    }
                },
                "required": ["function_name"]
            }),
        ),
        Tool::function(
            "find_callees",
            "Find all functions that a given function calls.",
            json!({
                "type": "object",
                "properties": {
                    "function_name": {
                        "type": "string",
                        "description": "Name of the function to find callees for"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results (default: 10)",
                        "default": 10
                    }
                },
                "required": ["function_name"]
            }),
        ),
        Tool::function(
            "recall",
            "Recall past decisions, context, or preferences stored in memory.",
            json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "What to search for in memory (e.g., 'authentication approach', 'database schema decisions')"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results (default: 5)",
                        "default": 5
                    }
                },
                "required": ["query"]
            }),
        ),
    ]
}

/// Execute a tool call and return the result
pub async fn execute_tool<C: ToolContext>(ctx: &C, tool_call: &ToolCall) -> String {
    let args: Value = serde_json::from_str(&tool_call.function.arguments).unwrap_or(json!({}));

    match tool_call.function.name.as_str() {
        "search_code" => {
            let query = args["query"].as_str().unwrap_or("");
            let limit = args["limit"].as_u64().unwrap_or(5) as usize;
            execute_search_code(ctx, query, limit).await
        }
        "get_symbols" => {
            let file_path = args["file_path"].as_str().unwrap_or("");
            execute_get_symbols(ctx, file_path).await
        }
        "read_file" => {
            let file_path = args["file_path"].as_str().unwrap_or("");
            let start_line = args["start_line"].as_u64().map(|n| n as usize);
            let end_line = args["end_line"].as_u64().map(|n| n as usize);
            execute_read_file(ctx, file_path, start_line, end_line).await
        }
        "find_callers" => {
            let function_name = args["function_name"].as_str().unwrap_or("");
            let limit = args["limit"].as_u64().unwrap_or(10) as usize;
            execute_find_callers(ctx, function_name, limit).await
        }
        "find_callees" => {
            let function_name = args["function_name"].as_str().unwrap_or("");
            let limit = args["limit"].as_u64().unwrap_or(10) as usize;
            execute_find_callees(ctx, function_name, limit).await
        }
        "recall" => {
            let query = args["query"].as_str().unwrap_or("");
            let limit = args["limit"].as_u64().unwrap_or(5) as usize;
            execute_recall(ctx, query, limit).await
        }
        _ => format!("Unknown tool: {}", tool_call.function.name),
    }
}

async fn execute_search_code<C: ToolContext>(ctx: &C, query: &str, limit: usize) -> String {
    let project_id = ctx.project_id().await;
    let project = ctx.get_project().await;
    let project_path = project.as_ref().map(|p| p.path.clone());

    match hybrid_search(
        ctx.pool(),
        ctx.embeddings(),
        query,
        project_id,
        project_path.as_deref(),
        limit,
    )
    .await
    {
        Ok(result) => {
            if result.results.is_empty() {
                "No code matches found.".to_string()
            } else {
                let mut output = format!("Found {} results:\n\n", result.results.len());
                for r in result.results {
                    // Truncate content if too long
                    let content_preview = if r.content.len() > 2000 {
                        format!("{}\n... (truncated)", &r.content[..2000])
                    } else {
                        r.content
                    };
                    output.push_str(&format!(
                        "### {}\n```\n{}\n```\n\n",
                        r.file_path, content_preview
                    ));
                }
                output
            }
        }
        Err(e) => format!("Search failed: {}", e),
    }
}

async fn execute_get_symbols<C: ToolContext>(ctx: &C, file_path: &str) -> String {
    let project = ctx.get_project().await;

    // Build full path
    let full_path = if let Some(ref proj) = project {
        if file_path.starts_with('/') {
            file_path.to_string()
        } else {
            format!("{}/{}", proj.path, file_path)
        }
    } else {
        file_path.to_string()
    };

    let path = Path::new(&full_path);
    if !path.exists() {
        return format!("File not found: {}", file_path);
    }

    match indexer::extract_symbols(path) {
        Ok(symbols) => {
            if symbols.is_empty() {
                format!("No symbols found in {}", file_path)
            } else {
                let mut output = format!("{} symbols in {}:\n", symbols.len(), file_path);
                for s in symbols.iter().take(50) {
                    // Increased limit slightly, but capped
                    let lines = if s.start_line == s.end_line {
                        format!("line {}", s.start_line)
                    } else {
                        format!("lines {}-{}", s.start_line, s.end_line)
                    };
                    output.push_str(&format!("  {} ({}) {}\n", s.name, s.symbol_type, lines));
                }
                if symbols.len() > 50 {
                    output.push_str(&format!("  ... and {} more\n", symbols.len() - 50));
                }
                output
            }
        }
        Err(e) => format!("Failed to get symbols: {}", e),
    }
}

async fn execute_read_file<C: ToolContext>(
    ctx: &C,
    file_path: &str,
    start_line: Option<usize>,
    end_line: Option<usize>,
) -> String {
    let project = ctx.get_project().await;

    // Build full path
    let full_path = if let Some(ref proj) = project {
        if file_path.starts_with('/') {
            file_path.to_string()
        } else {
            format!("{}/{}", proj.path, file_path)
        }
    } else {
        file_path.to_string()
    };

    match std::fs::read_to_string(&full_path) {
        Ok(content) => {
            let lines: Vec<&str> = content.lines().collect();
            let start = start_line.unwrap_or(1).saturating_sub(1);
            let mut end = end_line.unwrap_or(lines.len()).min(lines.len());

            // Cap output at 2000 lines max
            let max_lines = 2000;
            let mut truncated = false;

            if end - start > max_lines {
                end = start + max_lines;
                truncated = true;
            }

            if start >= lines.len() {
                return format!(
                    "Start line {} exceeds file length ({})",
                    start + 1,
                    lines.len()
                );
            }

            let selected: Vec<String> = lines[start..end]
                .iter()
                .enumerate()
                .map(|(i, line)| format!("{:4} | {}", start + i + 1, line))
                .collect();

            let mut output = format!("{}:\n{}", file_path, selected.join("\n"));
            if truncated {
                output.push_str("\n... (truncated, use start_line/end_line to read more)");
            }
            output
        }
        Err(e) => format!("Failed to read {}: {}", file_path, e),
    }
}

async fn execute_find_callers<C: ToolContext>(
    ctx: &C,
    function_name: &str,
    limit: usize,
) -> String {
    let project_id = ctx.project_id().await;
    let fn_name = function_name.to_string();

    let callers = ctx
        .pool()
        .run(move |conn| Ok::<_, String>(find_callers(conn, project_id, &fn_name, limit)))
        .await
        .unwrap_or_default();

    if callers.is_empty() {
        format!("No callers found for `{}`", function_name)
    } else {
        let mut output = format!("Functions that call `{}`:\n", function_name);
        for caller in callers {
            output.push_str(&format!(
                "  {} in {} ({}x)\n",
                caller.symbol_name, caller.file_path, caller.call_count
            ));
        }
        output
    }
}

async fn execute_find_callees<C: ToolContext>(
    ctx: &C,
    function_name: &str,
    limit: usize,
) -> String {
    let project_id = ctx.project_id().await;
    let fn_name = function_name.to_string();

    let callees = ctx
        .pool()
        .run(move |conn| Ok::<_, String>(find_callees(conn, project_id, &fn_name, limit)))
        .await
        .unwrap_or_default();

    if callees.is_empty() {
        format!("No callees found for `{}`", function_name)
    } else {
        let mut output = format!("Functions that `{}` calls:\n", function_name);
        for callee in callees {
            output.push_str(&format!(
                "  {} ({}x)\n",
                callee.symbol_name, callee.call_count
            ));
        }
        output
    }
}

async fn execute_recall<C: ToolContext>(ctx: &C, query: &str, limit: usize) -> String {
    let project_id = ctx.project_id().await;

    // Try semantic recall if embeddings available
    if let Some(embeddings) = ctx.embeddings() {
        if let Ok(query_embedding) = embeddings.embed(query).await {
            let embedding_bytes = embedding_to_bytes(&query_embedding);

            // Run vector search via connection pool
            let results: Result<Vec<(i64, String, f32)>, String> = ctx
                .pool()
                .run(move |conn| {
                    recall_semantic_sync(conn, &embedding_bytes, project_id, None, limit)
                })
                .await;

            if let Ok(results) = results {
                if !results.is_empty() {
                    let mut output = format!("Found {} relevant memories:\n\n", results.len());
                    for (id, content, distance) in results {
                        let score = 1.0 - distance;
                        let preview = if content.len() > 150 {
                            format!("{}...", &content[..150])
                        } else {
                            content
                        };
                        output.push_str(&format!("[{}] (score: {:.2}) {}\n", id, score, preview));
                    }
                    return output;
                }
            }
        }
    }

    // Fallback to keyword search via connection pool
    let query_owned = query.to_string();
    let result = ctx
        .pool()
        .run(move |conn| search_memories_sync(conn, project_id, &query_owned, None, limit))
        .await;

    match result {
        Ok(memories) => {
            if memories.is_empty() {
                "No relevant memories found.".to_string()
            } else {
                let mut output = format!("Found {} memories:\n\n", memories.len());
                for mem in memories {
                    let preview = if mem.content.len() > 150 {
                        format!("{}...", &mem.content[..150])
                    } else {
                        mem.content
                    };
                    output.push_str(&format!("[{}] {}\n", mem.id, preview));
                }
                output
            }
        }
        Err(e) => format!("Recall failed: {}", e),
    }
}
