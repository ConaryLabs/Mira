// src/operations/external_tools.rs
// External tool schemas for web search, URL fetching, and command execution

use serde_json::Value;

use crate::operations::tool_builder::{properties, ToolBuilder};

/// Get all external operation tool schemas
pub fn get_external_tools() -> Vec<Value> {
    vec![
        web_search(),
        fetch_url(),
        execute_command(),
    ]
}

/// Search the web for information
fn web_search() -> Value {
    ToolBuilder::new(
        "web_search",
        "Search the web for information. Returns search results with titles, URLs, and snippets.",
    )
    .property("query", properties::query("Search query"), true)
    .property("num_results", properties::integer("Number of results to return", Some(5)), false)
    .property(
        "search_type",
        properties::enum_string(
            "Type of search to perform",
            &["general", "documentation", "stackoverflow", "github"],
        ),
        false,
    )
    .build()
}

/// Fetch content from a URL
fn fetch_url() -> Value {
    ToolBuilder::new(
        "fetch_url",
        "Fetch content from a URL and extract text/code. Returns the extracted content.",
    )
    .property("url", properties::url("Full URL to fetch"), true)
    .property(
        "extract_mode",
        properties::enum_string(
            "What content to extract",
            &["full", "main_content", "code_blocks"],
        ),
        false,
    )
    .build()
}

/// Execute a shell command
fn execute_command() -> Value {
    ToolBuilder::new(
        "execute_command",
        "Execute a shell command and return the output. Use for build commands, tests, version checks, etc.",
    )
    .property("command", properties::string("Shell command to execute"), true)
    .property("working_directory", properties::path("Working directory (relative to project root)"), false)
    .property("timeout_seconds", properties::integer("Timeout in seconds", Some(30)), false)
    .build()
}
