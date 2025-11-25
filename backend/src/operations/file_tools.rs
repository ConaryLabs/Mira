// src/operations/file_tools.rs
// File operation tool schemas for GPT 5.1

use serde_json::Value;

use super::tool_builder::{ToolBuilder, properties};

/// Get all file operation tool schemas for GPT 5.1
pub fn get_file_operation_tools() -> Vec<Value> {
    vec![
        read_file_tool(),
        write_file_tool(),
        list_files_tool(),
        grep_files_tool(),
        summarize_file_tool(),
        extract_symbols_tool(),
        count_lines_tool(),
    ]
}

/// Tool: read_file
/// Reads the content of a file from the project directory
fn read_file_tool() -> Value {
    ToolBuilder::new(
        "read_file",
        "Read the content of a file from the project directory. Use this to examine existing code, configuration files, or documentation."
    )
    .property(
        "path",
        properties::path("Relative path to the file within the project (e.g., 'src/main.rs', 'package.json')"),
        true
    )
    .build()
}

/// Tool: write_file
/// Writes content to a file in the project directory
fn write_file_tool() -> Value {
    ToolBuilder::new(
        "write_file",
        "Write content to a file in the project directory. Creates new files or overwrites existing ones. Use this to save generated code or update configurations."
    )
    .property(
        "path",
        properties::path("Relative path to the file within the project (e.g., 'src/utils/helper.ts')"),
        true
    )
    .property(
        "content",
        properties::description("Complete file content to write"),
        true
    )
    .build()
}

/// Tool: list_files
/// Lists files in a directory with optional pattern matching
fn list_files_tool() -> Value {
    ToolBuilder::new(
        "list_files",
        "List files in a directory, optionally filtered by a glob pattern. Use this to discover project structure or find specific file types."
    )
    .property(
        "directory",
        properties::path("Directory path to list files from (e.g., 'src', 'src/components')"),
        true
    )
    .property(
        "pattern",
        properties::optional_string("Optional glob pattern to filter files (e.g., '*.ts', '**/*.tsx', '*.{js,ts}')"),
        false
    )
    .property(
        "recursive",
        properties::boolean("Whether to recursively list subdirectories", false),
        false
    )
    .build()
}

/// Tool: grep_files
/// Search for patterns in files using regex
fn grep_files_tool() -> Value {
    ToolBuilder::new(
        "grep_files",
        "Search for text patterns in project files using regex. Use this to find function definitions, imports, specific code patterns, or TODO comments."
    )
    .property(
        "pattern",
        properties::description("Regex pattern to search for (e.g., 'function.*export', 'class\\s+\\w+', 'TODO:')"),
        true
    )
    .property(
        "path",
        properties::optional_string("Optional directory or file path to search in (defaults to entire project)"),
        false
    )
    .property(
        "file_pattern",
        properties::optional_string("Optional glob pattern to filter which files to search (e.g., '*.ts', '**/*.rs')"),
        false
    )
    .property(
        "case_insensitive",
        properties::boolean("Whether the search should be case-insensitive", false),
        false
    )
    .build()
}

/// Tool: summarize_file
/// Get a high-level summary of a file without reading full content (saves tokens)
fn summarize_file_tool() -> Value {
    ToolBuilder::new(
        "summarize_file",
        "Get a summary of a file's structure and purpose without reading the entire content. Returns first/last N lines, file stats, and detected patterns. Use this instead of read_file when you only need to understand what the file does, not read all the code."
    )
    .property(
        "path",
        properties::path("Relative path to the file (e.g., 'src/main.rs')"),
        true
    )
    .property(
        "preview_lines",
        properties::optional_string("Number of lines to preview from start and end (default: 10)"),
        false
    )
    .build()
}

/// Tool: extract_symbols
/// Extract function/class/type definitions without full file content (saves tokens)
fn extract_symbols_tool() -> Value {
    ToolBuilder::new(
        "extract_symbols",
        "Extract symbols (functions, classes, types, interfaces) from a file without reading full content. Returns a structured list of definitions with signatures. Use this to understand file structure or find specific functions without loading the entire file."
    )
    .property(
        "path",
        properties::path("Relative path to the file (e.g., 'src/api/handlers.ts')"),
        true
    )
    .property(
        "symbol_types",
        properties::string_array("Types of symbols to extract (e.g., ['function', 'class', 'interface', 'type'])"),
        false
    )
    .build()
}

/// Tool: count_lines
/// Get file statistics without reading content (minimal tokens)
fn count_lines_tool() -> Value {
    ToolBuilder::new(
        "count_lines",
        "Get file statistics (line count, character count, file size) without reading content. Use this when you only need to know file size or complexity metrics."
    )
    .property(
        "paths",
        properties::string_array("List of file paths to get stats for"),
        true
    )
    .build()
}

// Tests in tests/phase5_providers_test.rs
