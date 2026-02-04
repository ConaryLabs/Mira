// background/pondering/types.rs
// Shared types for pondering submodules

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ToolUsageEntry {
    pub tool_name: String,
    pub arguments_summary: String,
    pub success: bool,
    pub timestamp: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct MemoryEntry {
    pub content: String,
    pub fact_type: String,
    pub category: Option<String>,
    pub status: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct PonderingInsight {
    pub pattern_type: String,
    pub description: String,
    pub confidence: f64,
    pub evidence: Vec<String>,
}
