// src/operations/tools/code_intelligence.rs
// Code intelligence tool schemas for AST-powered code analysis

use serde_json::Value;
use crate::operations::tool_builder::{ToolBuilder, properties};
use super::common::{with_project_id, with_limit, with_include_private, with_include_tests, with_optional_file_path};

/// Get all code intelligence tool schemas
pub fn get_tools() -> Vec<Value> {
    vec![
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
    ]
}

/// Find function/method definitions by name or pattern
pub fn find_function_tool() -> Value {
    let builder = ToolBuilder::new(
        "find_function",
        "Find function or method definitions by name or pattern. Supports wildcards (%) for flexible matching. Returns functions with their signatures, locations, complexity scores, and documentation. Essential for exploring unknown codebases or finding specific implementations."
    )
    .property("name", properties::pattern("Function name or pattern (use % as wildcard, e.g. 'handle%' finds handleClick, handleSubmit)"), true);

    let builder = with_project_id(builder, true);
    let builder = with_include_tests(builder, false);
    let builder = builder.property("min_complexity", properties::integer("Filter by minimum complexity score", None), false);
    with_limit(builder, 20).build()
}

/// Find class/struct/enum definitions
pub fn find_class_or_struct_tool() -> Value {
    let builder = ToolBuilder::new(
        "find_class_or_struct",
        "Find class, struct, or enum definitions by name. Returns type definitions with their methods, visibility, and documentation. Perfect for understanding data structures and object models."
    )
    .property("name", properties::pattern("Type name or pattern (supports % wildcard)"), true);

    let builder = with_project_id(builder, true);
    let builder = with_include_private(builder, false);
    with_limit(builder, 20).build()
}

/// Semantic code search using natural language
pub fn search_code_semantic_tool() -> Value {
    let builder = ToolBuilder::new(
        "search_code_semantic",
        "Semantic search across codebase using natural language. Uses vector embeddings to find relevant code based on meaning, not just keywords. Ask questions like 'authentication middleware' or 'error handling utilities' and get semantically relevant results."
    )
    .property("query", properties::query("Natural language description of what to find (e.g., 'authentication middleware', 'error handling utilities')"), true);

    let builder = with_project_id(builder, true);
    with_limit(builder, 10).build()
}

/// Find imports/usage of a symbol
pub fn find_imports_tool() -> Value {
    let builder = ToolBuilder::new(
        "find_imports",
        "Find where a symbol is imported or used across the codebase. Shows all files that import a specific function, class, or module. Essential for impact analysis and understanding dependencies."
    )
    .property("symbol", properties::string("Symbol to find (e.g., 'useState', 'HashMap', 'express')"), true);

    let builder = with_project_id(builder, true);
    with_limit(builder, 50).build()
}

/// Analyze external dependencies
pub fn analyze_dependencies_tool() -> Value {
    let builder = ToolBuilder::new(
        "analyze_dependencies",
        "Analyze external dependencies for a file or entire project. Shows npm packages, local imports, and standard library usage. Helps understand project structure and identify dependency issues."
    );

    let builder = with_project_id(builder, true);
    let builder = with_optional_file_path(builder, "Specific file to analyze (optional, omit for project-wide analysis)");
    builder.property(
        "group_by",
        properties::enum_string("How to group results", &["type", "frequency"]),
        false,
    ).build()
}

/// Get complexity hotspots
pub fn get_complexity_hotspots_tool() -> Value {
    let builder = ToolBuilder::new(
        "get_complexity_hotspots",
        "Find the most complex functions in the codebase. High complexity (cyclomatic complexity > 10) indicates code that may be hard to maintain and test. Use this to identify refactoring candidates."
    );

    let builder = with_project_id(builder, true);
    let builder = builder.property("min_complexity", properties::integer("Minimum complexity score to include", Some(10)), false);
    with_limit(builder, 10).build()
}

/// Get code quality issues
pub fn get_quality_issues_tool() -> Value {
    let builder = ToolBuilder::new(
        "get_quality_issues",
        "Get code quality issues for a file or project. Includes complexity problems, missing documentation, and potential bugs. Provides auto-fix suggestions when available."
    );

    let builder = with_project_id(builder, true);
    let builder = with_optional_file_path(builder, "Specific file to analyze (optional, omit for project-wide)");
    let builder = builder.property(
        "severity",
        properties::enum_string("Filter by severity", &["critical", "high", "medium", "low", "info"]),
        false,
    );
    let builder = builder.property(
        "issue_type",
        properties::enum_string("Filter by issue type", &["complexity", "documentation", "security"]),
        false,
    );
    with_limit(builder, 50).build()
}

/// Get all symbols in a file
pub fn get_file_symbols_tool() -> Value {
    let builder = ToolBuilder::new(
        "get_file_symbols",
        "Get all symbols (functions, classes, types) in a specific file. Returns structured overview of file contents, organized by symbol type. Essential for understanding file structure without reading full source."
    )
    .property("file_path", properties::path("Path to file to analyze"), true);

    let builder = with_project_id(builder, true);
    let builder = with_include_private(builder, true);
    builder.property("include_content", properties::boolean("Include full source code of elements (default: only signatures)", false), false).build()
}

/// Find tests for a code element
pub fn find_tests_for_code_tool() -> Value {
    let builder = ToolBuilder::new(
        "find_tests_for_code",
        "Find test files and test functions related to a code element. Helps verify test coverage and find relevant tests when modifying code."
    )
    .property("element_name", properties::string("Function or class name to find tests for"), true);

    let builder = with_project_id(builder, true);
    with_optional_file_path(builder, "Source file path (optional, helps narrow search)").build()
}

/// Get codebase statistics
pub fn get_codebase_stats_tool() -> Value {
    let builder = ToolBuilder::new(
        "get_codebase_stats",
        "Get comprehensive statistics about the codebase. Includes file counts, element counts (functions, classes), complexity metrics, test coverage estimates, and quality summary. Perfect for codebase health overview."
    );

    let builder = with_project_id(builder, true);
    builder.property(
        "breakdown_by",
        properties::enum_string("How to break down stats", &["language", "file_type", "complexity"]),
        false,
    ).build()
}

/// Find all callers of a function
pub fn find_callers_tool() -> Value {
    let builder = ToolBuilder::new(
        "find_callers",
        "Find all places where a function is called. Useful for impact analysis before refactoring - shows you everywhere that will be affected by changing a function's signature or behavior."
    )
    .property("function_name", properties::string("Function name to find callers for"), true);

    let builder = with_project_id(builder, true);
    with_limit(builder, 50).build()
}

/// Get full definition of a code element
pub fn get_element_definition_tool() -> Value {
    let builder = ToolBuilder::new(
        "get_element_definition",
        "Get the full definition of a code element (function, class, type) including signature, full source code, documentation, complexity score, and metadata. Use this to deeply understand a specific piece of code."
    )
    .property("element_name", properties::string("Name of element to get definition for"), true);

    let builder = with_project_id(builder, true);
    with_optional_file_path(builder, "File path to narrow search (optional)").build()
}
