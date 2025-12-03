// src/operations/tools/common.rs
// Shared property helpers for tool definitions

use serde_json::Value;
use crate::operations::tool_builder::{ToolBuilder, properties};

/// Add project_id property (required for code intelligence tools)
pub fn with_project_id(builder: ToolBuilder, required: bool) -> ToolBuilder {
    builder.property(
        "project_id",
        properties::string("Project ID to search/analyze within"),
        required,
    )
}

/// Add limit property with configurable default
pub fn with_limit(builder: ToolBuilder, default: i64) -> ToolBuilder {
    builder.property(
        "limit",
        properties::integer(
            &format!("Maximum results to return (default: {})", default),
            Some(default),
        ),
        false,
    )
}

/// Add optional file_path property
pub fn with_optional_file_path(builder: ToolBuilder, description: &str) -> ToolBuilder {
    builder.property("file_path", properties::path(description), false)
}

/// Add include_private property
pub fn with_include_private(builder: ToolBuilder, default: bool) -> ToolBuilder {
    builder.property(
        "include_private",
        properties::boolean("Include private/internal elements", default),
        false,
    )
}

/// Add include_tests property
pub fn with_include_tests(builder: ToolBuilder, default: bool) -> ToolBuilder {
    builder.property(
        "include_tests",
        properties::boolean("Include test functions in results", default),
        false,
    )
}

/// Standard enum for search types
pub fn search_type_enum() -> Value {
    serde_json::json!({
        "type": "string",
        "enum": ["general", "documentation", "stackoverflow", "github"],
        "description": "Type of search:\n- general: Broad web search\n- documentation: Focus on official docs\n- stackoverflow: Focus on Stack Overflow\n- github: Focus on GitHub repos and issues"
    })
}

/// Standard enum for extract modes
pub fn extract_mode_enum() -> Value {
    serde_json::json!({
        "type": "string",
        "enum": ["full", "main_content", "code_blocks"],
        "description": "What to extract:\n- full: All text content\n- main_content: Just the main article/doc content\n- code_blocks: Only code examples"
    })
}

/// Standard enum for task actions
pub fn task_action_enum() -> Value {
    serde_json::json!({
        "type": "string",
        "enum": ["create", "update", "complete", "list"],
        "description": "What to do:\n- create: Start tracking a new task\n- update: Add progress notes to existing task\n- complete: Mark task as done\n- list: Show all incomplete tasks"
    })
}

/// Standard enum for guidelines actions
pub fn guidelines_action_enum() -> Value {
    serde_json::json!({
        "type": "string",
        "enum": ["get", "set", "append"],
        "description": "What to do:\n- get: Retrieve current guidelines\n- set: Replace entire guidelines content\n- append: Add content to existing guidelines"
    })
}

/// Standard enum for priority levels
pub fn priority_enum() -> Value {
    serde_json::json!({
        "type": "string",
        "enum": ["low", "medium", "high", "critical"],
        "description": "Task priority (default: medium)"
    })
}

/// Standard enum for skill names
pub fn skill_name_enum() -> Value {
    serde_json::json!({
        "type": "string",
        "enum": ["refactoring", "testing", "debugging", "documentation"],
        "description": "Which specialized skill to activate:\n- refactoring: Systematic code improvement while preserving behavior\n- testing: Comprehensive test generation with best practices\n- debugging: Root cause analysis and systematic bug fixing\n- documentation: Clear, comprehensive documentation generation"
    })
}
