//! Shared types for tool execution

use serde::{Deserialize, Serialize};

/// Diff information for file modifications
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffInfo {
    pub path: String,
    pub old_content: Option<String>,
    pub new_content: String,
    pub is_new_file: bool,
}

/// Rich tool result with diff information for file operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RichToolResult {
    pub success: bool,
    pub output: String,
    pub diff: Option<DiffInfo>,
}
