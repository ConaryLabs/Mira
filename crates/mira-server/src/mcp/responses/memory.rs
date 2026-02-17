// crates/mira-server/src/mcp/responses/memory.rs

use schemars::JsonSchema;
use serde::Serialize;

use super::ToolOutput;

pub type MemoryOutput = ToolOutput<MemoryData>;

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum MemoryData {
    Remember(RememberData),
    Recall(RecallData),
    List(ListData),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct RememberData {
    pub id: i64,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct RecallData {
    pub memories: Vec<MemoryItem>,
    pub total: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ListData {
    pub memories: Vec<ListMemoryItem>,
    pub total: usize,
    pub offset: usize,
    pub has_more: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ListMemoryItem {
    pub id: i64,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fact_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct MemoryItem {
    pub id: i64,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fact_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
}
