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
    Export(ExportData),
    Purge(PurgeData),
    Entities(EntitiesData),
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

#[derive(Debug, Serialize, JsonSchema)]
pub struct ExportData {
    pub memories: Vec<ExportMemoryItem>,
    pub total: usize,
    pub project_name: Option<String>,
    pub exported_at: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ExportMemoryItem {
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    pub confidence: f64,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct PurgeData {
    pub deleted_count: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct EntitiesData {
    pub entities: Vec<EntityItem>,
    pub total: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct EntityItem {
    pub id: i64,
    pub canonical_name: String,
    pub entity_type: String,
    pub display_name: String,
    pub linked_facts: i64,
}
