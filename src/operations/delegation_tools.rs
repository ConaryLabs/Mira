// src/operations/delegation_tools.rs
// Tool schema definitions for GPT-5 delegation to DeepSeek

use serde_json::{json, Value};

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
    json!({
        "type": "function",
        "name": "generate_code",
        "description": "Generate a new code file from scratch. Use this when the user wants to create new functionality, components, or utilities.",
        "parameters": {
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path where the code should be created (e.g., 'src/components/Button.tsx')"
                },
                "description": {
                    "type": "string",
                    "description": "Clear description of what the code should do, including requirements, constraints, and expected behavior"
                },
                "language": {
                    "type": "string",
                    "enum": ["typescript", "javascript", "rust", "python", "go", "java", "cpp"],
                    "description": "Programming language for the generated code"
                },
                "framework": {
                    "type": "string",
                    "description": "Optional framework or library context (e.g., 'react', 'nextjs', 'axum', 'fastapi')"
                },
                "dependencies": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "List of external dependencies the code should use"
                },
                "style_guide": {
                    "type": "string",
                    "description": "Optional style preferences (e.g., 'functional', 'object-oriented', 'use async/await')"
                }
            },
            "required": ["path", "description", "language"]
        }
    })
}

/// Tool: refactor_code
/// Modifies existing code
fn refactor_code_tool() -> Value {
    json!({
        "type": "function",
        "name": "refactor_code",
        "description": "Refactor or modify existing code. Use this when improving, optimizing, or restructuring code that already exists.",
        "parameters": {
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path of the code to refactor"
                },
                "current_code": {
                    "type": "string",
                    "description": "The existing code that needs to be refactored"
                },
                "changes_requested": {
                    "type": "string",
                    "description": "Specific changes, improvements, or refactoring goals"
                },
                "language": {
                    "type": "string",
                    "enum": ["typescript", "javascript", "rust", "python", "go", "java", "cpp"],
                    "description": "Programming language of the code"
                },
                "preserve_behavior": {
                    "type": "boolean",
                    "description": "Whether to maintain exact same behavior (true) or allow behavioral improvements (false)",
                    "default": true
                }
            },
            "required": ["path", "current_code", "changes_requested", "language"]
        }
    })
}

/// Tool: debug_code
/// Fixes bugs or errors in code
fn debug_code_tool() -> Value {
    json!({
        "type": "function",
        "name": "debug_code",
        "description": "Debug and fix errors in code. Use this when there are specific bugs, errors, or issues that need resolution.",
        "parameters": {
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path of the buggy code"
                },
                "buggy_code": {
                    "type": "string",
                    "description": "The code that contains bugs or errors"
                },
                "error_message": {
                    "type": "string",
                    "description": "Error message, stack trace, or description of the bug"
                },
                "language": {
                    "type": "string",
                    "enum": ["typescript", "javascript", "rust", "python", "go", "java", "cpp"],
                    "description": "Programming language of the code"
                },
                "expected_behavior": {
                    "type": "string",
                    "description": "What the code should do when working correctly"
                }
            },
            "required": ["path", "buggy_code", "error_message", "language"]
        }
    })
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
