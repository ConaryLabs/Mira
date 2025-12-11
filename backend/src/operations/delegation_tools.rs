// src/operations/delegation_tools.rs
// Tool schema definitions for LLM tool calling
//
// This file is a thin wrapper around the modular tool definitions in tools/
// It maintains backward compatibility with existing code that imports from here.

use serde_json::Value;

// Re-export from tools module for backward compatibility
pub use super::tools::{
    get_delegation_tools, get_delegation_tools_with_mcp, get_llm_tools, get_llm_tools_with_mcp,
};

/// Parse tool call arguments from LLM response
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
