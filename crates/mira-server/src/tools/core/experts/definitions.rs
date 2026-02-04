// crates/mira-server/src/tools/core/experts/definitions.rs
// Tool definitions and static sets for expert sub-agents

use crate::llm::Tool;
use serde_json::json;
use std::sync::LazyLock;

/// Helper: define a tool with a query + optional limit parameter.
pub(super) fn query_tool(name: &str, desc: &str, query_desc: &str, default_limit: u64) -> Tool {
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
pub(super) fn function_name_tool(name: &str, desc: &str, fn_desc: &str, default_limit: u64) -> Tool {
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
pub(super) static EXPERT_TOOLS: LazyLock<Vec<Tool>> = LazyLock::new(|| {
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

pub(super) static STORE_FINDING_TOOL: LazyLock<Tool> = LazyLock::new(|| {
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

pub(super) static WEB_FETCH_TOOL: LazyLock<Tool> = LazyLock::new(|| {
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

pub(super) static WEB_SEARCH_TOOL: LazyLock<Tool> = LazyLock::new(|| {
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
