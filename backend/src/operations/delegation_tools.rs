// src/operations/delegation_tools.rs
// Tool schema definitions for GPT 5.1 tool calling
// Refactored to use ToolBuilder for cleaner, more maintainable code

use serde_json::Value;

use super::tool_builder::{ToolBuilder, properties};

/// Get all delegation tool schemas for GPT-5
/// Includes both code generation tools and file operation meta-tools
pub fn get_delegation_tools() -> Vec<Value> {
    vec![
        // Code generation delegation tools
        generate_code_tool(),
        refactor_code_tool(),
        debug_code_tool(),

        // File operation tools
        read_project_file_tool(),
        write_project_file_tool(),
        edit_project_file_tool(),
        search_codebase_tool(),
        list_project_files_tool(),

        // Token-optimized file operations (cheap alternatives)
        get_file_summary_tool(),
        get_file_structure_tool(),

        // External tools - web and command execution
        web_search_tool(),
        fetch_url_tool(),
        execute_command_tool(),

        // Git analysis tools - code history and collaboration
        git_history_tool(),
        git_blame_tool(),
        git_diff_tool(),
        git_file_history_tool(),
        git_branches_tool(),
        git_show_commit_tool(),
        git_file_at_commit_tool(),
        git_recent_changes_tool(),
        git_contributors_tool(),
        git_status_tool(),

        // Code intelligence tools - AST-powered code analysis
        find_function_tool(),
        find_class_or_struct_tool(),
        search_code_semantic_tool(),
        find_imports_tool(),
        analyze_dependencies_tool(),
        get_complexity_hotspots_tool(),
        get_quality_issues_tool(),
        get_file_symbols_tool(),
        find_tests_for_code_tool(),
        get_codebase_stats_tool(),
        find_callers_tool(),
        get_element_definition_tool(),

        // Skills system - specialized task handling
        activate_skill_tool(),
    ]
}

/// Get tool schemas for GPT 5.1 (executable tools for tool calling loop)
/// These are the actual tools GPT 5.1 can execute
pub fn get_gpt5_tools() -> Vec<Value> {
    vec![
        // File operation tools
        read_project_file_tool(),
        write_project_file_tool(),
        write_file_tool(), // NEW: Unrestricted file writing
        edit_project_file_tool(),
        search_codebase_tool(),
        list_project_files_tool(),

        // Token-optimized file operations
        get_file_summary_tool(),
        get_file_structure_tool(),

        // External tools - web and command execution
        web_search_tool(),
        fetch_url_tool(),
        execute_command_tool(),

        // Git analysis tools
        git_history_tool(),
        git_blame_tool(),
        git_diff_tool(),
        git_file_history_tool(),
        git_branches_tool(),
        git_show_commit_tool(),
        git_file_at_commit_tool(),
        git_recent_changes_tool(),
        git_contributors_tool(),
        git_status_tool(),

        // Code intelligence tools
        find_function_tool(),
        find_class_or_struct_tool(),
        search_code_semantic_tool(),
        find_imports_tool(),
        analyze_dependencies_tool(),
        get_complexity_hotspots_tool(),
        get_quality_issues_tool(),
        get_file_symbols_tool(),
        find_tests_for_code_tool(),
        get_codebase_stats_tool(),
        find_callers_tool(),
        get_element_definition_tool(),

        // Skills system
        activate_skill_tool(),

        // Project task management
        manage_project_task_tool(),

        // Project guidelines management
        manage_project_guidelines_tool(),
    ]
}

/// Tool: generate_code
/// Creates a new code file from scratch
fn generate_code_tool() -> Value {
    ToolBuilder::new(
        "generate_code",
        "Generate a new code file from scratch. Use this when the user wants to create new functionality, components, or utilities."
    )
    .property(
        "path",
        properties::path("File path where the code should be created (e.g., 'src/components/Button.tsx')"),
        true
    )
    .property(
        "description",
        properties::description("Clear description of what the code should do, including requirements, constraints, and expected behavior"),
        true
    )
    .property("language", properties::language(), true)
    .property(
        "framework",
        properties::optional_string("Optional framework or library context (e.g., 'react', 'nextjs', 'axum', 'fastapi')"),
        false
    )
    .property(
        "dependencies",
        properties::string_array("List of external dependencies the code should use"),
        false
    )
    .property(
        "style_guide",
        properties::optional_string("Optional style preferences (e.g., 'functional', 'object-oriented', 'use async/await')"),
        false
    )
    .build()
}

/// Tool: refactor_code
/// Modifies existing code
fn refactor_code_tool() -> Value {
    ToolBuilder::new(
        "refactor_code",
        "Refactor or modify existing code. Use this when improving, optimizing, or restructuring code that already exists."
    )
    .property(
        "path",
        properties::path("File path of the code to refactor"),
        true
    )
    .property(
        "current_code",
        properties::description("The existing code that needs to be refactored"),
        true
    )
    .property(
        "changes_requested",
        properties::description("Specific changes, improvements, or refactoring goals"),
        true
    )
    .property("language", properties::language(), true)
    .property(
        "preserve_behavior",
        properties::boolean("Whether to maintain exact same behavior (true) or allow behavioral improvements (false)", true),
        false
    )
    .build()
}

/// Tool: debug_code
/// Fixes bugs or errors in code
fn debug_code_tool() -> Value {
    ToolBuilder::new(
        "debug_code",
        "Debug and fix errors in code. Use this when there are specific bugs, errors, or issues that need resolution."
    )
    .property(
        "path",
        properties::path("File path of the buggy code"),
        true
    )
    .property(
        "buggy_code",
        properties::description("The code that contains bugs or errors"),
        true
    )
    .property(
        "error_message",
        properties::description("Error message, stack trace, or description of the bug"),
        true
    )
    .property("language", properties::language(), true)
    .property(
        "expected_behavior",
        properties::optional_string("What the code should do when working correctly"),
        false
    )
    .build()
}

/// Parse tool call arguments from GPT-5 response
/// Returns (tool_name, parsed_args)
pub fn parse_tool_call(tool_call: &Value) -> anyhow::Result<(String, Value)> {
    let tool_name = tool_call
        .get("function")
        .and_then(|f| f.get("name"))
        .and_then(|n| n.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing tool name in tool call"))?
        .to_string();

    let args_str = tool_call
        .get("function")
        .and_then(|f| f.get("arguments"))
        .and_then(|a| a.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing arguments in tool call"))?;

    let args: Value = serde_json::from_str(args_str)?;

    Ok((tool_name, args))
}

// ============================================================================
// File Operation Tools
// ============================================================================

/// Tool: read_project_file
/// Read files from the project
fn read_project_file_tool() -> Value {
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
/// Write files to the project
fn write_project_file_tool() -> Value {
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

/// Tool: write_file
/// Write to ANY file on the system (not restricted to project)
fn write_file_tool() -> Value {
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
        properties::optional_string("Create parent directories if they don't exist (default: true)"),
        false
    )
    .build()
}

/// Tool: edit_project_file
/// Edit files in the project using search/replace
fn edit_project_file_tool() -> Value {
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
/// Search for code patterns across the project
fn search_codebase_tool() -> Value {
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
/// List files in the project directory
fn list_project_files_tool() -> Value {
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

/// Tool: get_file_summary
/// Meta-tool for cheap file overview (uses summarize_file + count_lines)
fn get_file_summary_tool() -> Value {
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
        properties::optional_string("Number of lines to preview from start/end of each file (default: 10)"),
        false
    )
    .build()
}

/// Tool: get_file_structure
/// Meta-tool for extracting symbols (uses extract_symbols)
fn get_file_structure_tool() -> Value {
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
        properties::boolean("Whether to include private/internal symbols (default: false)", false),
        false
    )
    .build()
}

// ============================================================================
// External Tools - Web Search and Command Execution
// ============================================================================

/// Tool: web_search
/// Search the web for documentation, examples, error messages, etc.
fn web_search_tool() -> Value {
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
        properties::optional_string("Number of results to return (default: 5, max: 10)"),
        false
    )
    .property(
        "search_type",
        serde_json::json!({
            "type": "string",
            "enum": ["general", "documentation", "stackoverflow", "github"],
            "description": "Type of search:\n- general: Broad web search\n- documentation: Focus on official docs\n- stackoverflow: Focus on Stack Overflow\n- github: Focus on GitHub repos and issues"
        }),
        false
    )
    .build()
}

/// Tool: fetch_url
/// Fetch and parse content from a specific URL
fn fetch_url_tool() -> Value {
    ToolBuilder::new(
        "fetch_url",
        "Fetch and extract content from a specific URL. Useful for reading documentation pages, GitHub files, API references, or any web content. Returns extracted text content, removing HTML/CSS/JS noise."
    )
    .property(
        "url",
        serde_json::json!({
            "type": "string",
            "description": "Full URL to fetch (must start with http:// or https://)"
        }),
        true
    )
    .property(
        "extract_mode",
        serde_json::json!({
            "type": "string",
            "enum": ["full", "main_content", "code_blocks"],
            "description": "What to extract:\n- full: All text content\n- main_content: Just the main article/doc content\n- code_blocks: Only code examples"
        }),
        false
    )
    .build()
}

/// Tool: execute_command
/// Execute shell commands (with safety restrictions)
fn execute_command_tool() -> Value {
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
        properties::optional_string("Working directory for command execution (absolute or relative path, defaults to /home/peter/mira)"),
        false
    )
    .property(
        "timeout_seconds",
        properties::optional_string("Maximum execution time in seconds (default: 30, max: 300)"),
        false
    )
    .property(
        "environment",
        serde_json::json!({
            "type": "object",
            "description": "Optional environment variables to set (e.g., {\"NODE_ENV\": \"development\"})"
        }),
        false
    )
    .build()
}

// ============================================================================
// Git Analysis Tools - Code History and Collaboration
// ============================================================================

/// Tool: git_history
fn git_history_tool() -> Value {
    ToolBuilder::new(
        "git_history",
        "Get commit history with author, date, and message. Filter by branch, author, file, or date range. Useful for understanding code evolution and finding when changes were made."
    )
    .property(
        "branch",
        properties::optional_string("Branch name (default: current branch)"),
        false
    )
    .property(
        "limit",
        properties::optional_string("Maximum commits to return (default: 20)"),
        false
    )
    .property(
        "author",
        properties::optional_string("Filter by author name or email"),
        false
    )
    .property(
        "file_path",
        properties::optional_string("Show only commits affecting this file"),
        false
    )
    .property(
        "since",
        properties::optional_string("Show commits since date (e.g., '2024-01-01', '1 week ago')"),
        false
    )
    .build()
}

/// Tool: git_blame
fn git_blame_tool() -> Value {
    ToolBuilder::new(
        "git_blame",
        "Show who last modified each line of a file with commit hash, author, and date. Perfect for understanding why code was changed and who to ask about it."
    )
    .property(
        "file_path",
        properties::path("Path to the file to blame"),
        true
    )
    .property(
        "start_line",
        properties::optional_string("Start line number (optional)"),
        false
    )
    .property(
        "end_line",
        properties::optional_string("End line number (optional)"),
        false
    )
    .build()
}

/// Tool: git_diff
fn git_diff_tool() -> Value {
    ToolBuilder::new(
        "git_diff",
        "Show differences between commits, branches, or working tree. Returns added/removed/modified lines. Useful for code review and understanding changes."
    )
    .property(
        "from",
        properties::optional_string("Commit hash or branch name to compare from"),
        false
    )
    .property(
        "to",
        properties::optional_string("Commit hash or branch to compare to (default: working tree)"),
        false
    )
    .property(
        "file_path",
        properties::optional_string("Show diff for specific file only"),
        false
    )
    .build()
}

/// Tool: git_file_history
fn git_file_history_tool() -> Value {
    ToolBuilder::new(
        "git_file_history",
        "Show all commits that modified a specific file, tracking renames and moves. Useful for understanding file evolution and finding when bugs were introduced."
    )
    .property(
        "file_path",
        properties::path("Path to the file"),
        true
    )
    .property(
        "limit",
        properties::optional_string("Maximum commits to return (default: 20)"),
        false
    )
    .build()
}

/// Tool: git_branches
fn git_branches_tool() -> Value {
    ToolBuilder::new(
        "git_branches",
        "List all branches with last commit info and ahead/behind counts. Useful for understanding branch status and finding stale branches."
    )
    .property(
        "include_remote",
        properties::optional_string("Include remote branches (default: false)"),
        false
    )
    .build()
}

/// Tool: git_show_commit
fn git_show_commit_tool() -> Value {
    ToolBuilder::new(
        "git_show_commit",
        "Show detailed information about a specific commit including full diff and all files changed. Useful for understanding what a commit did."
    )
    .property(
        "commit_hash",
        properties::description("Commit hash (full or short)"),
        true
    )
    .build()
}

/// Tool: git_file_at_commit
fn git_file_at_commit_tool() -> Value {
    ToolBuilder::new(
        "git_file_at_commit",
        "Get the content of a file as it existed at a specific commit. Compare with current version to see how it changed. Useful for debugging when code broke."
    )
    .property(
        "file_path",
        properties::path("Path to the file"),
        true
    )
    .property(
        "commit_hash",
        properties::description("Commit hash or branch name"),
        true
    )
    .build()
}

/// Tool: git_recent_changes
fn git_recent_changes_tool() -> Value {
    ToolBuilder::new(
        "git_recent_changes",
        "Show files modified in the last N commits or days. Highlights frequently changed files (hot spots) that may need attention. Useful for finding volatile code."
    )
    .property(
        "days",
        properties::optional_string("Number of days to look back (default: 7)"),
        false
    )
    .property(
        "limit",
        properties::optional_string("Maximum commits to analyze (default: 50)"),
        false
    )
    .build()
}

/// Tool: git_contributors
fn git_contributors_tool() -> Value {
    ToolBuilder::new(
        "git_contributors",
        "Show who has contributed to the codebase with commit counts. Optionally filter by file/directory or date range. Useful for finding domain experts."
    )
    .property(
        "file_path",
        properties::optional_string("Show contributors for specific file or directory"),
        false
    )
    .property(
        "since",
        properties::optional_string("Show contributions since date (e.g., '1 month ago')"),
        false
    )
    .build()
}

/// Tool: git_status
fn git_status_tool() -> Value {
    ToolBuilder::new(
        "git_status",
        "Show current working tree status: staged, unstaged, and untracked files. Also shows current branch and sync status with remote. Essential for understanding current repository state."
    )
    .build()
}

// ============================================================================
// Code Intelligence Tools - AST-Powered Analysis
// ============================================================================

/// Tool: find_function
fn find_function_tool() -> Value {
    ToolBuilder::new(
        "find_function",
        "Find function or method definitions by name or pattern. Supports wildcards (%) for flexible matching. Returns functions with their signatures, locations, complexity scores, and documentation. Essential for exploring unknown codebases or finding specific implementations."
    )
    .property(
        "name",
        properties::description("Function name or pattern (use % as wildcard, e.g. 'handle%' finds handleClick, handleSubmit)"),
        true
    )
    .property(
        "project_id",
        properties::description("Project ID to search within"),
        true
    )
    .property(
        "include_tests",
        properties::optional_string("Include test functions in results (default: false)"),
        false
    )
    .property(
        "min_complexity",
        properties::optional_string("Filter by minimum complexity score"),
        false
    )
    .property(
        "limit",
        properties::optional_string("Maximum results to return (default: 20)"),
        false
    )
    .build()
}

/// Tool: find_class_or_struct
fn find_class_or_struct_tool() -> Value {
    ToolBuilder::new(
        "find_class_or_struct",
        "Find class, struct, or enum definitions by name. Returns type definitions with their methods, visibility, and documentation. Perfect for understanding data structures and object models."
    )
    .property(
        "name",
        properties::description("Type name or pattern (supports % wildcard)"),
        true
    )
    .property(
        "project_id",
        properties::description("Project ID to search within"),
        true
    )
    .property(
        "include_private",
        properties::optional_string("Include private/internal types (default: false)"),
        false
    )
    .property(
        "limit",
        properties::optional_string("Maximum results to return (default: 20)"),
        false
    )
    .build()
}

/// Tool: search_code_semantic
fn search_code_semantic_tool() -> Value {
    ToolBuilder::new(
        "search_code_semantic",
        "Semantic search across codebase using natural language. Uses vector embeddings to find relevant code based on meaning, not just keywords. Ask questions like 'authentication middleware' or 'error handling utilities' and get semantically relevant results."
    )
    .property(
        "query",
        properties::description("Natural language description of what to find (e.g., 'authentication middleware', 'error handling utilities')"),
        true
    )
    .property(
        "project_id",
        properties::description("Project ID to search within"),
        true
    )
    .property(
        "limit",
        properties::optional_string("Maximum results to return (default: 10)"),
        false
    )
    .build()
}

/// Tool: find_imports
fn find_imports_tool() -> Value {
    ToolBuilder::new(
        "find_imports",
        "Find where a symbol is imported or used across the codebase. Shows all files that import a specific function, class, or module. Essential for impact analysis and understanding dependencies."
    )
    .property(
        "symbol",
        properties::description("Symbol to find (e.g., 'useState', 'HashMap', 'express')"),
        true
    )
    .property(
        "project_id",
        properties::description("Project ID to search within"),
        true
    )
    .property(
        "limit",
        properties::optional_string("Maximum results to return (default: 50)"),
        false
    )
    .build()
}

/// Tool: analyze_dependencies
fn analyze_dependencies_tool() -> Value {
    ToolBuilder::new(
        "analyze_dependencies",
        "Analyze external dependencies for a file or entire project. Shows npm packages, local imports, and standard library usage. Helps understand project structure and identify dependency issues."
    )
    .property(
        "project_id",
        properties::description("Project ID to analyze"),
        true
    )
    .property(
        "file_path",
        properties::optional_string("Specific file to analyze (optional, omit for project-wide analysis)"),
        false
    )
    .property(
        "group_by",
        properties::optional_string("How to group results: 'type' (npm/local/std) or 'frequency' (most used first). Default: 'type'"),
        false
    )
    .build()
}

/// Tool: get_complexity_hotspots
fn get_complexity_hotspots_tool() -> Value {
    ToolBuilder::new(
        "get_complexity_hotspots",
        "Find the most complex functions in the codebase. High complexity (cyclomatic complexity > 10) indicates code that may be hard to maintain and test. Use this to identify refactoring candidates."
    )
    .property(
        "project_id",
        properties::description("Project ID to analyze"),
        true
    )
    .property(
        "min_complexity",
        properties::optional_string("Minimum complexity score to include (default: 10)"),
        false
    )
    .property(
        "limit",
        properties::optional_string("Maximum results to return (default: 10)"),
        false
    )
    .build()
}

/// Tool: get_quality_issues
fn get_quality_issues_tool() -> Value {
    ToolBuilder::new(
        "get_quality_issues",
        "Get code quality issues for a file or project. Includes complexity problems, missing documentation, and potential bugs. Provides auto-fix suggestions when available."
    )
    .property(
        "project_id",
        properties::description("Project ID to analyze"),
        true
    )
    .property(
        "file_path",
        properties::optional_string("Specific file to analyze (optional, omit for project-wide)"),
        false
    )
    .property(
        "severity",
        properties::optional_string("Filter by severity: 'critical', 'high', 'medium', 'low', 'info'"),
        false
    )
    .property(
        "issue_type",
        properties::optional_string("Filter by type: 'complexity', 'documentation', 'security'"),
        false
    )
    .property(
        "limit",
        properties::optional_string("Maximum results to return (default: 50)"),
        false
    )
    .build()
}

/// Tool: get_file_symbols
fn get_file_symbols_tool() -> Value {
    ToolBuilder::new(
        "get_file_symbols",
        "Get all symbols (functions, classes, types) in a specific file. Returns structured overview of file contents, organized by symbol type. Essential for understanding file structure without reading full source."
    )
    .property(
        "file_path",
        properties::path("Path to file to analyze"),
        true
    )
    .property(
        "project_id",
        properties::description("Project ID"),
        true
    )
    .property(
        "include_private",
        properties::optional_string("Include private/internal symbols (default: true)"),
        false
    )
    .property(
        "include_content",
        properties::optional_string("Include full source code of elements (default: false, only signatures)"),
        false
    )
    .build()
}

/// Tool: find_tests_for_code
fn find_tests_for_code_tool() -> Value {
    ToolBuilder::new(
        "find_tests_for_code",
        "Find test files and test functions related to a code element. Helps verify test coverage and find relevant tests when modifying code."
    )
    .property(
        "element_name",
        properties::description("Function or class name to find tests for"),
        true
    )
    .property(
        "project_id",
        properties::description("Project ID"),
        true
    )
    .property(
        "file_path",
        properties::optional_string("Source file path (optional, helps narrow search)"),
        false
    )
    .build()
}

/// Tool: get_codebase_stats
fn get_codebase_stats_tool() -> Value {
    ToolBuilder::new(
        "get_codebase_stats",
        "Get comprehensive statistics about the codebase. Includes file counts, element counts (functions, classes), complexity metrics, test coverage estimates, and quality summary. Perfect for codebase health overview."
    )
    .property(
        "project_id",
        properties::description("Project ID to analyze"),
        true
    )
    .property(
        "breakdown_by",
        properties::optional_string("How to break down stats: 'language', 'file_type', or 'complexity'. Default: 'language'"),
        false
    )
    .build()
}

/// Tool: find_callers
fn find_callers_tool() -> Value {
    ToolBuilder::new(
        "find_callers",
        "Find all places where a function is called. Useful for impact analysis before refactoring - shows you everywhere that will be affected by changing a function's signature or behavior."
    )
    .property(
        "function_name",
        properties::description("Function name to find callers for"),
        true
    )
    .property(
        "project_id",
        properties::description("Project ID"),
        true
    )
    .property(
        "limit",
        properties::optional_string("Maximum results to return (default: 50)"),
        false
    )
    .build()
}

/// Tool: get_element_definition
fn get_element_definition_tool() -> Value {
    ToolBuilder::new(
        "get_element_definition",
        "Get the full definition of a code element (function, class, type) including signature, full source code, documentation, complexity score, and metadata. Use this to deeply understand a specific piece of code."
    )
    .property(
        "element_name",
        properties::description("Name of element to get definition for"),
        true
    )
    .property(
        "project_id",
        properties::description("Project ID"),
        true
    )
    .property(
        "file_path",
        properties::optional_string("File path to narrow search (optional)"),
        false
    )
    .build()
}

// ============================================================================
// Skills System - Specialized Task Handling
// ============================================================================

/// Tool: activate_skill
/// Activates a specialized skill for complex tasks
fn activate_skill_tool() -> Value {
    ToolBuilder::new(
        "activate_skill",
        "Activate a specialized skill for complex tasks like refactoring, testing, debugging, or documentation. Skills provide expert guidance, best practices, and restrict available tools to what's relevant for the task. Use this when you need systematic, step-by-step guidance for non-trivial tasks."
    )
    .property(
        "skill_name",
        serde_json::json!({
            "type": "string",
            "enum": ["refactoring", "testing", "debugging", "documentation"],
            "description": "Which specialized skill to activate:\n- refactoring: Systematic code improvement while preserving behavior\n- testing: Comprehensive test generation with best practices\n- debugging: Root cause analysis and systematic bug fixing\n- documentation: Clear, comprehensive documentation generation"
        }),
        true
    )
    .property(
        "task_description",
        properties::description("Detailed description of what you want to accomplish with this skill"),
        true
    )
    .property(
        "context",
        properties::optional_string("Additional context about the code, project, or requirements that the skill should know about"),
        false
    )
    .build()
}

// ============================================================================
// Project Task Management
// ============================================================================

/// Tool: manage_project_task
/// Create, update, or complete persistent project tasks
fn manage_project_task_tool() -> Value {
    ToolBuilder::new(
        "manage_project_task",
        "Create, update, or complete persistent project tasks. Tasks persist across sessions and track your progress on work items. Use this when:
        - User requests new work (feature, fix, improvement) -> create a task
        - You're continuing work on an existing task -> update with progress notes
        - Work is finished -> complete the task with a summary

        Tasks automatically link to artifacts and commits you produce."
    )
    .property(
        "action",
        serde_json::json!({
            "type": "string",
            "enum": ["create", "update", "complete", "list"],
            "description": "What to do:\n- create: Start tracking a new task\n- update: Add progress notes to existing task\n- complete: Mark task as done\n- list: Show all incomplete tasks"
        }),
        true
    )
    .property(
        "task_id",
        serde_json::json!({
            "type": "integer",
            "description": "Task ID (required for update/complete actions)"
        }),
        false
    )
    .property(
        "title",
        properties::description("Task title - short description of what needs to be done (required for create)"),
        false
    )
    .property(
        "description",
        properties::optional_string("Detailed description of the task or progress update"),
        false
    )
    .property(
        "priority",
        serde_json::json!({
            "type": "string",
            "enum": ["low", "medium", "high", "critical"],
            "description": "Task priority (for create, default: medium)"
        }),
        false
    )
    .property(
        "progress_notes",
        properties::optional_string("Progress update or completion summary (for update/complete)"),
        false
    )
    .property(
        "tags",
        properties::string_array("Tags for categorization (e.g., ['feature', 'frontend'])"),
        false
    )
    .build()
}

/// Tool: manage_project_guidelines
/// Create, view, or update project guidelines that persist across sessions
fn manage_project_guidelines_tool() -> Value {
    ToolBuilder::new(
        "manage_project_guidelines",
        "Create or update project guidelines that persist across sessions. Guidelines are automatically included in every conversation about this project. Use this when:
        - User asks to initialize or setup project context (like 'claude init')
        - User wants to document coding standards, preferences, or architecture
        - User asks to view or update existing guidelines

        Guidelines help maintain consistency across conversations and provide context about the project."
    )
    .property(
        "action",
        serde_json::json!({
            "type": "string",
            "enum": ["get", "set", "append"],
            "description": "What to do:\n- get: Retrieve current guidelines\n- set: Replace entire guidelines content\n- append: Add content to existing guidelines"
        }),
        true
    )
    .property(
        "content",
        serde_json::json!({
            "type": "string",
            "description": "Guidelines content in markdown format (required for set/append)"
        }),
        false
    )
    .property(
        "section",
        serde_json::json!({
            "type": "string",
            "description": "Section heading to add (for append action, creates a new ## section)"
        }),
        false
    )
    .build()
}

// Tests in tests/phase5_providers_test.rs
