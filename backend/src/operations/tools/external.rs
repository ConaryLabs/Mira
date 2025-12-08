// src/operations/tools/external.rs
// External tool schemas for web search, URL fetching, and command execution

use serde_json::Value;
use crate::operations::tool_builder::{ToolBuilder, properties};
use super::common::{search_type_enum, extract_mode_enum};

/// Get all external operation tool schemas
pub fn get_tools() -> Vec<Value> {
    vec![
        web_search_tool(),
        fetch_url_tool(),
        execute_command_tool(),
        run_tests_tool(),
    ]
}

/// Tool: web_search
pub fn web_search_tool() -> Value {
    ToolBuilder::new(
        "web_search",
        "Search the web for documentation, API references, error messages, code examples, or any other information needed for coding tasks. Useful for finding latest library documentation, troubleshooting errors, or discovering best practices."
    )
    .property(
        "query",
        properties::description("Search query - be specific and include relevant keywords (e.g., 'rust tokio spawn error', 'react hooks useEffect cleanup')"),
        true
    )
    .property(
        "num_results",
        properties::integer("Number of results to return (max: 10)", Some(5)),
        false
    )
    .property("search_type", search_type_enum(), false)
    .build()
}

/// Tool: fetch_url
pub fn fetch_url_tool() -> Value {
    ToolBuilder::new(
        "fetch_url",
        "Fetch and extract content from a specific URL. Useful for reading documentation pages, GitHub files, API references, or any web content. Returns extracted text content, removing HTML/CSS/JS noise."
    )
    .property("url", properties::url("Full URL to fetch (must start with http:// or https://)"), true)
    .property("extract_mode", extract_mode_enum(), false)
    .build()
}

/// Tool: execute_command
pub fn execute_command_tool() -> Value {
    ToolBuilder::new(
        "execute_command",
        "Execute ANY shell command on the system. Use this for system administration (restart services, edit configs, install packages), build commands (npm install, cargo build), or any command-line operations. Supports sudo for privileged operations. IMPORTANT: You have full system access - use it to help the user manage their system."
    )
    .property(
        "command",
        properties::description("Shell command to execute. Can include sudo for privileged operations (e.g., 'sudo systemctl restart nginx', 'echo \"Hello\" > /tmp/test.txt', 'npm install lodash')"),
        true
    )
    .property(
        "working_directory",
        properties::path("Working directory for command execution (absolute or relative path)"),
        false
    )
    .property(
        "timeout_seconds",
        properties::integer("Maximum execution time in seconds (max: 300)", Some(30)),
        false
    )
    .property(
        "environment",
        properties::optional_string("Optional environment variables as JSON string (e.g., '{\"NODE_ENV\": \"development\"}')"),
        false
    )
    .build()
}

/// Tool: run_tests
/// Enables the agentic development loop - LLM can run tests, see failures, fix code, and re-run
pub fn run_tests_tool() -> Value {
    ToolBuilder::new(
        "run_tests",
        "Run the mira-test test suite to validate code changes. Use this to verify your changes work correctly. Returns structured JSON with pass/fail status, failure details, and suggestions for fixes. Supports running all scenarios or specific ones. Use --mock flag for fast validation without real LLM calls."
    )
    .property(
        "scenario",
        properties::optional_string("Specific scenario file to run (e.g., 'smoke_test.yaml', 'file_operations.yaml'). Omit to run all scenarios."),
        false
    )
    .property(
        "tags",
        properties::optional_string("Comma-separated tags to filter scenarios (e.g., 'smoke,fast'). Only scenarios with matching tags will run."),
        false
    )
    .property(
        "mock",
        properties::boolean("Run in mock mode (no real LLM calls, instant execution). Useful for testing the test framework itself.", false),
        false
    )
    .property(
        "timeout_seconds",
        properties::integer("Maximum time to wait for all tests to complete (max: 600)", Some(300)),
        false
    )
    .build()
}
