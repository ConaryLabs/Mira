// crates/mira-server/src/tools/core/experts/tools.rs
// Tool dispatch and execution for expert sub-agents

use super::ToolContext;
use super::definitions::{EXPERT_TOOLS, STORE_FINDING_TOOL, WEB_FETCH_TOOL, WEB_SEARCH_TOOL};
use super::web::{execute_web_fetch, execute_web_search, has_brave_search};
use crate::db::{recall_semantic_sync, search_memories_sync};
use crate::indexer;
use crate::llm::{Tool, ToolCall};
use crate::search::embedding_to_bytes;
use crate::utils::{truncate, truncate_at_boundary};
use crate::tools::core::code::{query_callees, query_callers, query_search_code};
use serde_json::{Value, json};
use std::path::Path;
use std::sync::Arc;
use tracing::debug;

/// Define the tools available to experts
pub fn get_expert_tools() -> Vec<Tool> {
    EXPERT_TOOLS.clone()
}

/// Build the full expert toolset: base tools + optionally store_finding + web tools + MCP tools.
///
/// Use `include_store_finding: true` for council mode (experts record findings).
pub async fn build_expert_toolset<C: ToolContext>(
    ctx: &C,
    include_store_finding: bool,
) -> Vec<Tool> {
    let mut tools = get_expert_tools();

    if include_store_finding {
        tools.push(STORE_FINDING_TOOL.clone());
    }

    tools.push(WEB_FETCH_TOOL.clone());

    if has_brave_search() {
        tools.push(WEB_SEARCH_TOOL.clone());
    }

    let mcp_tools = ctx.mcp_expert_tools().await;
    if !mcp_tools.is_empty() {
        debug!(
            mcp_tool_count = mcp_tools.len(),
            "Adding MCP tools to expert tool set"
        );
        tools.extend(mcp_tools);
    }

    debug!(total_tools = tools.len(), "Expert tool set built");
    tools
}

/// Execute a tool call during council mode, with access to the FindingsStore.
/// Falls through to `execute_tool` for all tools except `store_finding`.
pub async fn execute_tool_with_findings<C: ToolContext>(
    ctx: &C,
    tool_call: &ToolCall,
    findings_store: &Arc<super::findings::FindingsStore>,
    role_key: &str,
) -> String {
    if tool_call.function.name == "store_finding" {
        let args: Value = serde_json::from_str(&tool_call.function.arguments).unwrap_or(json!({}));
        return execute_store_finding(&args, findings_store, role_key);
    }
    execute_tool(ctx, tool_call).await
}

/// Handle the store_finding tool call by writing to the FindingsStore.
fn execute_store_finding(
    args: &Value,
    store: &Arc<super::findings::FindingsStore>,
    role: &str,
) -> String {
    use super::findings::{AddFindingResult, CouncilFinding};

    let topic = args["topic"].as_str().unwrap_or("").to_string();
    let content = args["content"].as_str().unwrap_or("").to_string();
    let severity = args["severity"].as_str().unwrap_or("info").to_string();
    let evidence: Vec<String> = args["evidence"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let recommendation = args["recommendation"].as_str().map(String::from);

    if topic.is_empty() || content.is_empty() {
        return "Error: 'topic' and 'content' are required".to_string();
    }

    let finding = CouncilFinding {
        role: role.to_string(),
        topic,
        content,
        evidence,
        severity,
        recommendation,
    };

    match store.add(finding) {
        AddFindingResult::Added { total } => format!("Finding recorded ({} total)", total),
        AddFindingResult::RoleLimitReached { role_count } => format!(
            "Your findings limit reached ({} stored). Focus on your most critical findings only.",
            role_count
        ),
        AddFindingResult::GlobalLimitReached { total } => format!(
            "Council findings limit reached ({} total). No more findings can be recorded.",
            total
        ),
    }
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
        "web_fetch" => {
            let url = args["url"].as_str().unwrap_or("");
            let max_chars = args["max_chars"].as_u64().unwrap_or(15000) as usize;
            execute_web_fetch(url, max_chars).await
        }
        "web_search" => {
            let query = args["query"].as_str().unwrap_or("");
            let count = args["count"].as_u64().unwrap_or(5).min(10) as u32;
            execute_web_search(query, count).await
        }
        name if name.starts_with("mcp__") => execute_mcp_tool(ctx, name, args).await,
        _ => format!("Unknown tool: {}", tool_call.function.name),
    }
}

async fn execute_search_code<C: ToolContext>(ctx: &C, query: &str, limit: usize) -> String {
    match query_search_code(ctx, query, limit).await {
        Ok(result) => {
            if result.results.is_empty() {
                "No code matches found.".to_string()
            } else {
                let mut output = format!("Found {} results:\n\n", result.results.len());
                for r in result.results {
                    let content_preview = if r.content.len() > 2000 {
                        format!("{}\n... (truncated)", truncate_at_boundary(&r.content, 2000))
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
    let callers = query_callers(ctx, function_name, limit).await;

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
    let callees = query_callees(ctx, function_name, limit).await;

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

/// Execute an MCP tool by delegating to the McpClientManager via ToolContext
async fn execute_mcp_tool<C: ToolContext>(ctx: &C, prefixed_name: &str, args: Value) -> String {
    // Parse the prefixed name: mcp__{server}__{tool_name}
    let parts: Vec<&str> = prefixed_name.splitn(3, "__").collect();
    if parts.len() != 3 || parts[0] != "mcp" {
        return format!("Error: Invalid MCP tool name format: {}", prefixed_name);
    }

    let server_name = parts[1];
    let tool_name = parts[2];

    debug!(server = server_name, tool = tool_name, "Executing MCP tool");

    // Get the MCP client manager from the context
    let mcp_tools = ctx.list_mcp_tools().await;
    if mcp_tools.is_empty() {
        return "Error: No MCP servers configured".to_string();
    }

    // Verify the server exists
    let server_exists = mcp_tools.iter().any(|(name, _)| name == server_name);
    if !server_exists {
        return format!(
            "Error: MCP server '{}' not found. Available: {}",
            server_name,
            mcp_tools
                .iter()
                .map(|(n, _)| n.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    // Call the MCP tool via the context's mcp_call_tool method
    match ctx.mcp_call_tool(server_name, tool_name, args).await {
        Ok(result) => result,
        Err(e) => format!(
            "Error calling MCP tool {}/{}: {}",
            server_name, tool_name, e
        ),
    }
}

async fn execute_recall<C: ToolContext>(ctx: &C, query: &str, limit: usize) -> String {
    let project_id = ctx.project_id().await;

    // Try semantic recall if embeddings available
    if let Some(embeddings) = ctx.embeddings()
        && let Ok(query_embedding) = embeddings.embed(query).await {
            let embedding_bytes = embedding_to_bytes(&query_embedding);

            // Run vector search via connection pool
            let results: Result<Vec<(i64, String, f32)>, String> = ctx
                .pool()
                .run(move |conn| {
                    recall_semantic_sync(conn, &embedding_bytes, project_id, None, limit)
                })
                .await;

            if let Ok(results) = results
                && !results.is_empty() {
                    let mut output = format!("Found {} relevant memories:\n\n", results.len());
                    for (id, content, distance) in results {
                        let score = 1.0 - distance;
                        let preview = truncate(&content, 150);
                        output.push_str(&format!("[{}] (score: {:.2}) {}\n", id, score, preview));
                    }
                    return output;
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
                    let preview = truncate(&mem.content, 150);
                    output.push_str(&format!("[{}] {}\n", mem.id, preview));
                }
                output
            }
        }
        Err(e) => format!("Recall failed: {}", e),
    }
}

#[cfg(test)]
mod tests {
    use super::super::definitions::{WEB_FETCH_TOOL, WEB_SEARCH_TOOL};

    #[test]
    fn test_web_fetch_tool_definition() {
        let tool = &*WEB_FETCH_TOOL;
        assert_eq!(tool.function.name, "web_fetch");
        assert!(tool.function.description.contains("Fetch"));
        let params = &tool.function.parameters;
        assert_eq!(params["type"], "object");
        assert!(params["properties"]["url"]["type"] == "string");
        let required = params["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v == "url"));
    }

    #[test]
    fn test_web_search_tool_definition() {
        let tool = &*WEB_SEARCH_TOOL;
        assert_eq!(tool.function.name, "web_search");
        assert!(tool.function.description.contains("Search"));
        let params = &tool.function.parameters;
        assert_eq!(params["type"], "object");
        assert!(params["properties"]["query"]["type"] == "string");
        let required = params["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v == "query"));
    }

    #[test]
    fn test_mcp_tool_name_parsing() {
        // Test valid MCP tool name parsing
        let name = "mcp__context7__resolve-library-id";
        let parts: Vec<&str> = name.splitn(3, "__").collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0], "mcp");
        assert_eq!(parts[1], "context7");
        assert_eq!(parts[2], "resolve-library-id");
    }

    #[test]
    fn test_mcp_tool_name_with_nested_underscores() {
        let name = "mcp__my_server__my_tool_name";
        let parts: Vec<&str> = name.splitn(3, "__").collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0], "mcp");
        assert_eq!(parts[1], "my_server");
        assert_eq!(parts[2], "my_tool_name");
    }
}
