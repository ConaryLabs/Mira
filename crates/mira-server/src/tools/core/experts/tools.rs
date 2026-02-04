// crates/mira-server/src/tools/core/experts/tools.rs
// Tool definitions and execution for expert sub-agents

use super::ToolContext;
use crate::db::{recall_semantic_sync, search_memories_sync};
use crate::indexer;
use crate::llm::{Tool, ToolCall};
use crate::search::embedding_to_bytes;
use crate::tools::core::code::{query_callers, query_callees, query_search_code};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::{Arc, LazyLock};
use tracing::debug;

/// Helper: define a tool with a query + optional limit parameter.
fn query_tool(name: &str, desc: &str, query_desc: &str, default_limit: u64) -> Tool {
    Tool::function(
        name,
        desc,
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": query_desc },
                "limit": { "type": "integer", "description": format!("Maximum number of results (default: {})", default_limit), "default": default_limit }
            },
            "required": ["query"]
        }),
    )
}

/// Helper: define a tool with a function_name + optional limit parameter.
fn function_name_tool(name: &str, desc: &str, fn_desc: &str, default_limit: u64) -> Tool {
    Tool::function(
        name,
        desc,
        json!({
            "type": "object",
            "properties": {
                "function_name": { "type": "string", "description": fn_desc },
                "limit": { "type": "integer", "description": format!("Maximum number of results (default: {})", default_limit), "default": default_limit }
            },
            "required": ["function_name"]
        }),
    )
}

/// Base tools available to all experts (built once, cloned per invocation).
static EXPERT_TOOLS: LazyLock<Vec<Tool>> = LazyLock::new(|| {
    vec![
        query_tool(
            "search_code",
            "Search for code by meaning. Use this to find relevant code snippets, functions, or patterns.",
            "Natural language description of what you're looking for (e.g., 'authentication middleware', 'error handling in API routes')",
            5,
        ),
        Tool::function(
            "get_symbols",
            "Get the structure of a file - lists all functions, structs, classes, etc.",
            json!({
                "type": "object",
                "properties": {
                    "file_path": { "type": "string", "description": "Path to the file (relative to project root)" }
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
                    "file_path": { "type": "string", "description": "Path to the file (relative to project root)" },
                    "start_line": { "type": "integer", "description": "Starting line number (1-indexed, optional)" },
                    "end_line": { "type": "integer", "description": "Ending line number (inclusive, optional)" }
                },
                "required": ["file_path"]
            }),
        ),
        function_name_tool(
            "find_callers",
            "Find all functions that call a given function.",
            "Name of the function to find callers for",
            10,
        ),
        function_name_tool(
            "find_callees",
            "Find all functions that a given function calls.",
            "Name of the function to find callees for",
            10,
        ),
        query_tool(
            "recall",
            "Recall past decisions, context, or preferences stored in memory.",
            "What to search for in memory (e.g., 'authentication approach', 'database schema decisions')",
            5,
        ),
    ]
});

static STORE_FINDING_TOOL: LazyLock<Tool> = LazyLock::new(|| {
    Tool::function(
        "store_finding",
        "Record a key finding from your analysis. Use this whenever you discover something significant.",
        json!({
            "type": "object",
            "properties": {
                "topic": { "type": "string", "description": "Brief topic name (e.g., 'error handling', 'auth flow')" },
                "content": { "type": "string", "description": "The finding itself — what you discovered" },
                "severity": { "type": "string", "enum": ["info", "low", "medium", "high", "critical"], "description": "Severity level of this finding (default: info)" },
                "evidence": { "type": "array", "items": { "type": "string" }, "description": "File paths, code snippets, or references supporting this finding" },
                "recommendation": { "type": "string", "description": "What to do about it (optional)" }
            },
            "required": ["topic", "content"]
        }),
    )
});

static WEB_FETCH_TOOL: LazyLock<Tool> = LazyLock::new(|| {
    Tool::function(
        "web_fetch",
        "Fetch a web page and extract its text content. Use this to read documentation, articles, or any web resource.",
        json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "The URL to fetch (must start with http:// or https://)" },
                "max_chars": { "type": "integer", "description": "Maximum characters to return (default: 15000)", "default": 15000 }
            },
            "required": ["url"]
        }),
    )
});

static WEB_SEARCH_TOOL: LazyLock<Tool> = LazyLock::new(|| {
    Tool::function(
        "web_search",
        "Search the web for current information using Brave Search. Use this to find documentation, recent articles, or answers to technical questions.",
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "The search query" },
                "count": { "type": "integer", "description": "Number of results to return (default: 5, max: 10)", "default": 5 }
            },
            "required": ["query"]
        }),
    )
});

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

/// Fetch a web page and extract text content
async fn execute_web_fetch(url: &str, max_chars: usize) -> String {
    if url.is_empty() {
        return "Error: URL is required".to_string();
    }

    // Validate URL
    let parsed = match url::Url::parse(url) {
        Ok(u) => u,
        Err(e) => return format!("Error: Invalid URL '{}': {}", url, e),
    };

    let scheme = parsed.scheme();
    if scheme != "http" && scheme != "https" {
        return format!(
            "Error: Only http:// and https:// URLs are supported, got {}",
            scheme
        );
    }

    // Build HTTP client with browser-like settings
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("Mozilla/5.0 (compatible; Mira/1.0; +https://github.com/ConaryLabs/Mira)")
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
    {
        Ok(c) => c,
        Err(e) => return format!("Error: Failed to create HTTP client: {}", e),
    };

    // Fetch the page
    let response = match client.get(url).send().await {
        Ok(r) => r,
        Err(e) => {
            if e.is_timeout() {
                return format!("Error: Request timed out fetching {}", url);
            }
            if e.is_connect() {
                return format!("Error: Could not connect to {}", url);
            }
            return format!("Error: Failed to fetch {}: {}", url, e);
        }
    };

    let status = response.status();
    if !status.is_success() {
        return format!("Error: HTTP {} when fetching {}", status.as_u16(), url);
    }

    // Check content type - only process text/html and text/plain
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();

    let is_html = content_type.contains("text/html");
    let is_text = content_type.contains("text/plain") || content_type.contains("text/markdown");

    if !is_html && !is_text && !content_type.is_empty() {
        return format!(
            "Error: Unsupported content type '{}' for {}. Only HTML and text content is supported.",
            content_type, url
        );
    }

    // Read body
    let body = match response.text().await {
        Ok(b) => b,
        Err(e) => return format!("Error: Failed to read response body from {}: {}", url, e),
    };

    // Extract text content
    let text = if is_html || content_type.is_empty() {
        extract_text_from_html(&body)
    } else {
        body
    };

    // Truncate if needed
    let truncated = if text.len() > max_chars {
        format!(
            "{}\n\n... (truncated at {} chars, total {})",
            &text[..max_chars],
            max_chars,
            text.len()
        )
    } else {
        text
    };

    format!("Content from {}:\n\n{}", url, truncated)
}

/// Extract readable text from HTML using scraper
fn extract_text_from_html(html: &str) -> String {
    use scraper::{Html, Selector};

    let document = Html::parse_document(html);

    // Try to get main content area first
    let selectors = [
        "main",
        "article",
        "[role=main]",
        ".content",
        "#content",
        "body",
    ];
    for sel_str in &selectors {
        if let Ok(selector) = Selector::parse(sel_str) {
            if let Some(element) = document.select(&selector).next() {
                let text = element.text().collect::<Vec<_>>().join(" ");
                let cleaned = clean_extracted_text(&text);
                if !cleaned.is_empty() && cleaned.len() > 100 {
                    return cleaned;
                }
            }
        }
    }

    // Fallback: get all text from body
    let text: String = document.root_element().text().collect::<Vec<_>>().join(" ");
    clean_extracted_text(&text)
}

/// Clean up extracted text - normalize whitespace and remove noise
fn clean_extracted_text(text: &str) -> String {
    // Split into lines, trim each, then reassemble with paragraph breaks
    let lines: Vec<&str> = text.lines().map(|l| l.trim()).collect();
    let mut result = String::with_capacity(text.len());
    let mut consecutive_empty = 0;

    for line in &lines {
        if line.is_empty() {
            consecutive_empty += 1;
            continue;
        }

        if !result.is_empty() {
            if consecutive_empty >= 1 {
                // Paragraph break (max 2 newlines)
                result.push_str("\n\n");
            } else {
                // Same paragraph - join with space
                result.push(' ');
            }
        }

        // Collapse internal whitespace within the line
        let mut last_was_space = false;
        for ch in line.chars() {
            if ch.is_whitespace() {
                if !last_was_space {
                    result.push(' ');
                    last_was_space = true;
                }
            } else {
                result.push(ch);
                last_was_space = false;
            }
        }

        consecutive_empty = 0;
    }

    result.trim().to_string()
}

/// Check if Brave Search API key is configured
pub fn has_brave_search() -> bool {
    std::env::var("BRAVE_API_KEY")
        .ok()
        .filter(|k| !k.trim().is_empty())
        .is_some()
}

/// Search the web using Brave Search API
async fn execute_web_search(query: &str, count: u32) -> String {
    if query.is_empty() {
        return "Error: Search query is required".to_string();
    }

    let api_key = match std::env::var("BRAVE_API_KEY") {
        Ok(key) if !key.trim().is_empty() => key,
        _ => return "Error: BRAVE_API_KEY not configured. Set it in ~/.mira/.env to enable web search.".to_string(),
    };

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
    {
        Ok(c) => c,
        Err(e) => return format!("Error: Failed to create HTTP client: {}", e),
    };

    let encoded_query = urlencoding::encode(query);
    let url = format!(
        "https://api.search.brave.com/res/v1/web/search?q={}&count={}&extra_snippets=true&text_decorations=false",
        encoded_query, count
    );

    let response = match client
        .get(&url)
        .header("X-Subscription-Token", &api_key)
        .header("Accept", "application/json")
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            if e.is_timeout() {
                return "Error: Brave Search request timed out".to_string();
            }
            return format!("Error: Brave Search request failed: {}", e);
        }
    };

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return format!(
            "Error: Brave Search returned HTTP {}: {}",
            status.as_u16(),
            body
        );
    }

    let body: Value = match response.json().await {
        Ok(v) => v,
        Err(e) => return format!("Error: Failed to parse Brave Search response: {}", e),
    };

    // Extract web results
    let results = match body["web"]["results"].as_array() {
        Some(r) => r,
        None => return "No search results found.".to_string(),
    };

    if results.is_empty() {
        return "No search results found.".to_string();
    }

    let mut output = format!("Search results for \"{}\":\n\n", query);
    for (i, result) in results.iter().enumerate() {
        let title = result["title"].as_str().unwrap_or("(no title)");
        let url = result["url"].as_str().unwrap_or("");
        let description = result["description"].as_str().unwrap_or("(no description)");

        output.push_str(&format!(
            "{}. **{}**\n   {}\n   {}\n",
            i + 1,
            title,
            url,
            description
        ));

        // Include extra snippets if available (richer context for experts)
        if let Some(snippets) = result["extra_snippets"].as_array() {
            for snippet in snippets.iter().take(2) {
                if let Some(s) = snippet.as_str() {
                    output.push_str(&format!("   > {}\n", s));
                }
            }
        }

        output.push('\n');
    }

    output
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_clean_extracted_text() {
        let input = "  Hello   World  \n\n\n\n  Foo  ";
        let result = clean_extracted_text(input);
        assert_eq!(result, "Hello World\n\nFoo");
    }

    #[test]
    fn test_clean_extracted_text_preserves_double_newline() {
        let input = "Paragraph one.\n\nParagraph two.";
        let result = clean_extracted_text(input);
        assert_eq!(result, "Paragraph one.\n\nParagraph two.");
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

    #[test]
    fn test_extract_text_from_html_basic() {
        let html = r#"<html><body><main><h1>Title</h1><p>Hello world</p></main></body></html>"#;
        let text = extract_text_from_html(html);
        assert!(text.contains("Title"));
        assert!(text.contains("Hello world"));
    }

    #[test]
    fn test_extract_text_from_html_article() {
        let html = r#"<html><body><nav>Menu</nav><article><h1>Article Title</h1><p>Article content here with enough text to pass the 100 char minimum threshold for content extraction.</p></article></body></html>"#;
        let text = extract_text_from_html(html);
        assert!(text.contains("Article Title"));
        assert!(text.contains("Article content"));
    }

    #[tokio::test]
    async fn test_execute_web_fetch_invalid_url() {
        let result = execute_web_fetch("not-a-url", 1000).await;
        assert!(result.starts_with("Error:"));
    }

    #[tokio::test]
    async fn test_execute_web_fetch_empty_url() {
        let result = execute_web_fetch("", 1000).await;
        assert_eq!(result, "Error: URL is required");
    }

    #[tokio::test]
    async fn test_execute_web_fetch_bad_scheme() {
        let result = execute_web_fetch("ftp://example.com", 1000).await;
        assert!(result.contains("Only http:// and https://"));
    }

    #[tokio::test]
    async fn test_execute_web_search_empty_query() {
        let result = execute_web_search("", 5).await;
        assert_eq!(result, "Error: Search query is required");
    }

    #[tokio::test]
    async fn test_execute_web_search_no_api_key() {
        // Only run this test if BRAVE_API_KEY is not set (to avoid unsafe env manipulation)
        if has_brave_search() {
            // Skip test when key is present - we don't want to manipulate env unsafely
            return;
        }
        let result = execute_web_search("test query", 5).await;
        assert!(result.contains("BRAVE_API_KEY not configured"));
    }
}
