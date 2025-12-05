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
        "Read content from project files. Returns max 500 lines by default. For large files, use offset/limit to read specific sections. Consider get_file_summary or get_file_structure first to understand file layout before reading full content."
    )
    .property(
        "paths",
        properties::string_array("List of file paths to read (e.g., ['src/main.rs', 'Cargo.toml'])"),
        true
    )
    .property(
        "offset",
        properties::integer("Starting line number (0-indexed). Use to skip to specific section.", None),
        false
    )
    .property(
        "limit",
        properties::integer("Maximum lines to read (default: 500). Use smaller values for large files.", Some(500)),
        false
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
        "Create a NEW file in the project. WARNING: For modifying EXISTING files, use edit_project_file instead - it's 85% more token-efficient. Only use write_project_file when creating brand new files that don't exist yet."
    )
    .property(
        "path",
        properties::path("File path for the NEW file (e.g., 'src/utils/helper.ts')"),
        true
    )
    .property(
        "content",
        properties::description("Complete file content for the new file."),
        true
    )
    .property(
        "purpose",
        properties::optional_string("Brief explanation of what this file does"),
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
        "PREFERRED method for modifying existing files. Uses exact string replacement (like a diff) - 85% more efficient than rewriting entire files. The search string must uniquely match exactly one location in the file. For multiple changes, call this tool multiple times."
    )
    .property(
        "path",
        properties::path("File path to edit (e.g., 'src/main.rs')"),
        true
    )
    .property(
        "search",
        properties::description("Exact text to find and replace. Must match EXACTLY including whitespace and indentation. Include enough context to uniquely identify the location."),
        true
    )
    .property(
        "replace",
        properties::description("New text to replace the search string with. Can be empty to delete text."),
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
