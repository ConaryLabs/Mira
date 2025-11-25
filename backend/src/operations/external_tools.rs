// src/operations/external_tools.rs
// External tool schemas for web search, URL fetching, and command execution

use serde_json::{json, Value};

/// Get all external operation tool schemas
pub fn get_external_tools() -> Vec<Value> {
    vec![
        web_search_internal_tool(),
        fetch_url_internal_tool(),
        execute_command_internal_tool(),
    ]
}

/// Internal web search tool
fn web_search_internal_tool() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "web_search_internal",
            "description": "Search the web for information. Returns search results with titles, URLs, and snippets.",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query"
                    },
                    "num_results": {
                        "type": "string",
                        "description": "Number of results (default: 5)"
                    },
                    "search_type": {
                        "type": "string",
                        "enum": ["general", "documentation", "stackoverflow", "github"],
                        "description": "Type of search to perform"
                    }
                },
                "required": ["query"]
            }
        }
    })
}

/// Internal URL fetch tool
fn fetch_url_internal_tool() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "fetch_url_internal",
            "description": "Fetch content from a URL and extract text/code. Returns the extracted content.",
            "parameters": {
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "Full URL to fetch"
                    },
                    "extract_mode": {
                        "type": "string",
                        "enum": ["full", "main_content", "code_blocks"],
                        "description": "What content to extract"
                    }
                },
                "required": ["url"]
            }
        }
    })
}

/// Internal command execution tool
fn execute_command_internal_tool() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "execute_command_internal",
            "description": "Execute a shell command and return the output. Use for build commands, tests, version checks, etc.",
            "parameters": {
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "Shell command to execute"
                    },
                    "working_directory": {
                        "type": "string",
                        "description": "Working directory (relative to project root)"
                    },
                    "timeout_seconds": {
                        "type": "string",
                        "description": "Timeout in seconds (default: 30)"
                    }
                },
                "required": ["command"]
            }
        }
    })
}
