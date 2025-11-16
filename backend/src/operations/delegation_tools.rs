// src/operations/delegation_tools.rs
// Tool schema definitions for GPT-5 delegation to DeepSeek
// Refactored to use ToolBuilder for cleaner, more maintainable code

use serde_json::Value;

use super::tool_builder::{ToolBuilder, properties};

/// Get all delegation tool schemas for GPT-5
pub fn get_delegation_tools() -> Vec<Value> {
    vec![
        generate_code_tool(),
        refactor_code_tool(),
        debug_code_tool(),
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

// Tests in tests/phase5_providers_test.rs
