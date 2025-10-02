// src/tools/types.rs
// Minimal tool types for prompt building compatibility

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    pub tool_type: String,
    pub function: Option<ToolFunction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolFunction {
    pub name: String,
    pub description: String,
    pub parameters: Option<serde_json::Value>,
}
