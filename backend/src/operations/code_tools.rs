// src/operations/code_tools.rs
// Code intelligence tool schemas for GPT 5.1

use serde_json::Value;

use crate::operations::tool_builder::{properties, ToolBuilder};

/// Get all code intelligence tool schemas
pub fn get_code_tools() -> Vec<Value> {
    vec![
        find_function(),
        find_class_or_struct(),
        search_code_semantic(),
        find_imports(),
        analyze_dependencies(),
        get_complexity_hotspots(),
        get_quality_issues(),
        get_file_symbols(),
        find_tests_for_code(),
        get_codebase_stats(),
        find_callers(),
        get_element_definition(),
    ]
}

/// Find function/method definitions by name or pattern
fn find_function() -> Value {
    ToolBuilder::new(
        "find_function",
        "Find function or method definitions by name or pattern. Supports wildcards for flexible matching.",
    )
    .property("name", properties::pattern("Function name or pattern"), true)
    .property("project_id", properties::string("Project ID to search within"), true)
    .property("include_tests", properties::boolean("Include test functions in results", false), false)
    .property("min_complexity", properties::integer("Filter by minimum complexity score", None), false)
    .property("limit", properties::integer("Maximum results to return", Some(20)), false)
    .build()
}

/// Find class/struct/enum definitions
fn find_class_or_struct() -> Value {
    ToolBuilder::new(
        "find_class_or_struct",
        "Find class, struct, or enum definitions by name. Returns type definitions with their methods and documentation.",
    )
    .property("name", properties::pattern("Type name or pattern"), true)
    .property("project_id", properties::string("Project ID to search within"), true)
    .property("include_private", properties::boolean("Include private/internal types", false), false)
    .property("limit", properties::integer("Maximum results to return", Some(20)), false)
    .build()
}

/// Semantic code search using natural language
fn search_code_semantic() -> Value {
    ToolBuilder::new(
        "search_code_semantic",
        "Semantic search across codebase using natural language. Uses vector embeddings to find relevant code based on meaning.",
    )
    .property("query", properties::query("Natural language description of what to find (e.g., 'authentication middleware', 'error handling utilities')"), true)
    .property("project_id", properties::string("Project ID to search within"), true)
    .property("limit", properties::integer("Maximum results to return", Some(10)), false)
    .build()
}

/// Find imports/usage of a symbol
fn find_imports() -> Value {
    ToolBuilder::new(
        "find_imports",
        "Find where a symbol is imported or used across the codebase. Shows all files that import a specific function, class, or module.",
    )
    .property("symbol", properties::string("Symbol to find (e.g., 'useState', 'HashMap', 'express')"), true)
    .property("project_id", properties::string("Project ID to search within"), true)
    .property("limit", properties::integer("Maximum results to return", Some(50)), false)
    .build()
}

/// Analyze external dependencies
fn analyze_dependencies() -> Value {
    ToolBuilder::new(
        "analyze_dependencies",
        "Analyze external dependencies for a file or entire project. Shows npm packages, local imports, and standard library usage.",
    )
    .property("project_id", properties::string("Project ID to analyze"), true)
    .property("file_path", properties::path("Specific file to analyze (optional, omit for project-wide analysis)"), false)
    .property(
        "group_by",
        properties::enum_string("How to group results", &["type", "frequency"]),
        false,
    )
    .build()
}

/// Get complexity hotspots
fn get_complexity_hotspots() -> Value {
    ToolBuilder::new(
        "get_complexity_hotspots",
        "Find the most complex functions in the codebase. High complexity indicates code that may be hard to maintain and test.",
    )
    .property("project_id", properties::string("Project ID to analyze"), true)
    .property("min_complexity", properties::integer("Minimum complexity score to include", Some(10)), false)
    .property("limit", properties::integer("Maximum results to return", Some(10)), false)
    .build()
}

/// Get code quality issues
fn get_quality_issues() -> Value {
    ToolBuilder::new(
        "get_quality_issues",
        "Get code quality issues for a file or project. Includes complexity problems, missing documentation, and auto-fix suggestions.",
    )
    .property("project_id", properties::string("Project ID to analyze"), true)
    .property("file_path", properties::path("Specific file to analyze (optional, omit for project-wide)"), false)
    .property(
        "severity",
        properties::enum_string("Filter by severity", &["critical", "high", "medium", "low", "info"]),
        false,
    )
    .property(
        "issue_type",
        properties::enum_string("Filter by issue type", &["complexity", "documentation", "security"]),
        false,
    )
    .property("limit", properties::integer("Maximum results to return", Some(50)), false)
    .build()
}

/// Get all symbols in a file
fn get_file_symbols() -> Value {
    ToolBuilder::new(
        "get_file_symbols",
        "Get all symbols (functions, classes, types) in a specific file. Returns structured overview of file contents.",
    )
    .property("file_path", properties::path("Path to file to analyze"), true)
    .property("project_id", properties::string("Project ID"), true)
    .property("include_private", properties::boolean("Include private/internal symbols", true), false)
    .property("include_content", properties::boolean("Include full source code (default: only signatures)", false), false)
    .build()
}

/// Find tests for a code element
fn find_tests_for_code() -> Value {
    ToolBuilder::new(
        "find_tests_for_code",
        "Find test files and test functions related to a code element. Helps verify test coverage.",
    )
    .property("element_name", properties::string("Function or class name to find tests for"), true)
    .property("project_id", properties::string("Project ID"), true)
    .property("file_path", properties::path("Source file path (optional, helps narrow search)"), false)
    .build()
}

/// Get codebase statistics
fn get_codebase_stats() -> Value {
    ToolBuilder::new(
        "get_codebase_stats",
        "Get comprehensive statistics about the codebase. Includes file counts, complexity metrics, test coverage, and quality summary.",
    )
    .property("project_id", properties::string("Project ID to analyze"), true)
    .property(
        "breakdown_by",
        properties::enum_string("How to break down stats", &["language", "file_type", "complexity"]),
        false,
    )
    .build()
}

/// Find all callers of a function
fn find_callers() -> Value {
    ToolBuilder::new(
        "find_callers",
        "Find all places where a function is called. Useful for impact analysis before refactoring.",
    )
    .property("function_name", properties::string("Function name to find callers for"), true)
    .property("project_id", properties::string("Project ID"), true)
    .property("limit", properties::integer("Maximum results to return", Some(50)), false)
    .build()
}

/// Get full definition of a code element
fn get_element_definition() -> Value {
    ToolBuilder::new(
        "get_element_definition",
        "Get the full definition of a code element (function, class, type) including signature, documentation, and metadata.",
    )
    .property("element_name", properties::string("Name of element to get definition for"), true)
    .property("project_id", properties::string("Project ID"), true)
    .property("file_path", properties::path("File path to narrow search (optional)"), false)
    .build()
}
