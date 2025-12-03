// src/operations/tools/file_operations.rs
// File operation tool schemas for reading, writing, and analyzing files

use serde_json::Value;
use crate::operations::tool_builder::{ToolBuilder, properties};

/// Get all file operation tool schemas
pub fn get_tools() -> Vec<Value> {
    vec![
        read_project_file_tool(),
        write_project_file_tool(),
        write_file_tool(),
        edit_project_file_tool(),
        search_codebase_tool(),
        list_project_files_tool(),
        get_file_summary_tool(),
        get_file_structure_tool(),
    ]
}

/// Get low-level file tools (used by meta-tools)
pub fn get_low_level_tools() -> Vec<Value> {
    vec![
        read_file_tool(),
        write_file_internal_tool(),
        list_files_tool(),
        grep_files_tool(),
        summarize_file_tool(),
        extract_symbols_tool(),
        count_lines_tool(),
    ]
}

// ============================================================================
// High-level file tools (exposed to LLM)
// ============================================================================

/// Tool: read_project_file
pub fn read_project_file_tool() -> Value {
    ToolBuilder::new(
        "read_project_file",
        "Read the content of one or more files from the project. Use this when you need to examine existing code, configuration, or documentation before generating new code or answering questions."
    )
    .property(
        "paths",
        properties::string_array("List of file paths to read (e.g., ['src/main.rs', 'Cargo.toml'])"),
        true
    )
    .property(
        "purpose",
        properties::description("Why you need to read these files (helps with context optimization)"),
        false
    )
    .build()
}

/// Tool: write_project_file
pub fn write_project_file_tool() -> Value {
    ToolBuilder::new(
        "write_project_file",
        "Write content to a file in the project. Creates new files or overwrites existing ones. Use this to save generated code, create new modules, or update configuration files. For partial edits to existing files, use edit_project_file instead."
    )
    .property(
        "path",
        properties::path("File path to write to (e.g., 'src/utils/helper.ts', 'config/settings.json')"),
        true
    )
    .property(
        "content",
        properties::description("Complete file content to write. For existing files, this will overwrite the entire file."),
        true
    )
    .property(
        "purpose",
        properties::optional_string("Brief explanation of what this file does (helps with documentation)"),
        false
    )
    .build()
}

/// Tool: write_file (unrestricted - can write anywhere)
pub fn write_file_tool() -> Value {
    ToolBuilder::new(
        "write_file",
        "Write content to ANY file on the system. Use this for creating files outside projects, system configuration files (nginx, systemd, etc.), temporary files, or any other file. Unlike write_project_file, this doesn't require project context. Full filesystem access."
    )
    .property(
        "path",
        properties::path("Absolute file path to write to (e.g., '/tmp/test.txt', '/etc/nginx/sites-available/mysite', '/home/peter/notes.txt')"),
        true
    )
    .property(
        "content",
        properties::description("Complete file content to write. This will overwrite the file if it exists."),
        true
    )
    .property(
        "create_dirs",
        properties::boolean("Create parent directories if they don't exist", true),
        false
    )
    .build()
}

/// Tool: edit_project_file
pub fn edit_project_file_tool() -> Value {
    ToolBuilder::new(
        "edit_project_file",
        "Make targeted edits to an existing file using search and replace. Use this when you need to modify specific parts of a file without rewriting the entire file. Safer than write_project_file for small changes."
    )
    .property(
        "path",
        properties::path("File path to edit (e.g., 'src/main.rs')"),
        true
    )
    .property(
        "search",
        properties::description("Exact text to search for (will be replaced). Must match exactly including whitespace."),
        true
    )
    .property(
        "replace",
        properties::description("Text to replace the search string with. Can be empty to delete text."),
        true
    )
    .property(
        "purpose",
        properties::optional_string("Brief explanation of what this edit accomplishes"),
        false
    )
    .build()
}

/// Tool: search_codebase
pub fn search_codebase_tool() -> Value {
    ToolBuilder::new(
        "search_codebase",
        "Search for code patterns, function definitions, imports, or specific text across the project. Use this to find where functionality is defined or how APIs are used."
    )
    .property(
        "query",
        properties::description("Search query - can be a regex pattern, function name, or plain text"),
        true
    )
    .property(
        "file_pattern",
        properties::optional_string("Optional glob pattern to limit search (e.g., '*.rs', 'src/**/*.ts')"),
        false
    )
    .property(
        "case_sensitive",
        properties::boolean("Whether search should be case-sensitive", true),
        false
    )
    .build()
}

/// Tool: list_project_files
pub fn list_project_files_tool() -> Value {
    ToolBuilder::new(
        "list_project_files",
        "List files in the project directory, optionally filtered by pattern. Use this to understand project structure or find specific file types."
    )
    .property(
        "directory",
        properties::path("Directory to list (e.g., 'src', 'src/components'). Use '.' for project root."),
        false
    )
    .property(
        "pattern",
        properties::optional_string("Optional glob pattern to filter files (e.g., '*.ts', '**/*.rs')"),
        false
    )
    .property(
        "recursive",
        properties::boolean("Whether to recursively list subdirectories", false),
        false
    )
    .build()
}

/// Tool: get_file_summary (meta-tool)
pub fn get_file_summary_tool() -> Value {
    ToolBuilder::new(
        "get_file_summary",
        "Get a quick overview of files without reading full content. Returns first/last 10 lines, file stats, and detected patterns. Use this instead of read_project_file when you only need to understand what files do, not read all the code. Saves 80-90% tokens compared to full read."
    )
    .property(
        "paths",
        properties::string_array("List of file paths to summarize (e.g., ['src/main.rs', 'lib/utils.ts'])"),
        true
    )
    .property(
        "preview_lines",
        properties::integer("Number of lines to preview from start/end of each file", Some(10)),
        false
    )
    .build()
}

/// Tool: get_file_structure (meta-tool)
pub fn get_file_structure_tool() -> Value {
    ToolBuilder::new(
        "get_file_structure",
        "Extract the structure (functions, classes, types, etc.) from files without reading full content. Returns a list of symbol definitions. Use this to understand code organization or find specific functions without loading entire files. Saves 70-80% tokens compared to full read."
    )
    .property(
        "paths",
        properties::string_array("List of file paths to extract structure from"),
        true
    )
    .property(
        "include_private",
        properties::boolean("Whether to include private/internal symbols", false),
        false
    )
    .build()
}

// ============================================================================
// Low-level file tools (used internally by meta-tools)
// ============================================================================

fn read_file_tool() -> Value {
    ToolBuilder::new(
        "read_file",
        "Read the content of a file from the project directory."
    )
    .property(
        "path",
        properties::path("Relative path to the file within the project"),
        true
    )
    .build()
}

fn write_file_internal_tool() -> Value {
    ToolBuilder::new(
        "write_file_internal",
        "Write content to a file in the project directory."
    )
    .property("path", properties::path("Relative path to the file"), true)
    .property("content", properties::description("Complete file content to write"), true)
    .build()
}

fn list_files_tool() -> Value {
    ToolBuilder::new(
        "list_files",
        "List files in a directory, optionally filtered by a glob pattern."
    )
    .property("directory", properties::path("Directory path to list files from"), true)
    .property("pattern", properties::optional_string("Optional glob pattern to filter files"), false)
    .property("recursive", properties::boolean("Whether to recursively list subdirectories", false), false)
    .build()
}

fn grep_files_tool() -> Value {
    ToolBuilder::new(
        "grep_files",
        "Search for text patterns in project files using regex."
    )
    .property("pattern", properties::description("Regex pattern to search for"), true)
    .property("path", properties::optional_string("Optional directory or file path to search in"), false)
    .property("file_pattern", properties::optional_string("Optional glob pattern to filter files"), false)
    .property("case_insensitive", properties::boolean("Whether the search should be case-insensitive", false), false)
    .build()
}

fn summarize_file_tool() -> Value {
    ToolBuilder::new(
        "summarize_file",
        "Get a summary of a file's structure and purpose without reading the entire content."
    )
    .property("path", properties::path("Relative path to the file"), true)
    .property("preview_lines", properties::integer("Number of lines to preview", Some(10)), false)
    .build()
}

fn extract_symbols_tool() -> Value {
    ToolBuilder::new(
        "extract_symbols",
        "Extract symbols (functions, classes, types) from a file without reading full content."
    )
    .property("path", properties::path("Relative path to the file"), true)
    .property("symbol_types", properties::string_array("Types of symbols to extract"), false)
    .build()
}

fn count_lines_tool() -> Value {
    ToolBuilder::new(
        "count_lines",
        "Get file statistics (line count, character count, file size) without reading content."
    )
    .property("paths", properties::string_array("List of file paths to get stats for"), true)
    .build()
}
