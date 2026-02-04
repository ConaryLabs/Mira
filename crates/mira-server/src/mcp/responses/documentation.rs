use schemars::JsonSchema;
use serde::Serialize;

use super::ToolOutput;

pub type DocOutput = ToolOutput<DocData>;

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum DocData {
    List(DocListData),
    Get(DocGetData),
    Inventory(DocInventoryData),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DocListData {
    pub tasks: Vec<DocTaskItem>,
    pub total: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DocTaskItem {
    pub id: i64,
    pub doc_category: String,
    pub target_doc_path: String,
    pub priority: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_file_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DocGetData {
    pub task_id: i64,
    pub target_doc_path: String,
    pub full_target_path: String,
    pub doc_type: String,
    pub doc_category: String,
    pub priority: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_file_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_source_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub guidelines: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DocInventoryData {
    pub docs: Vec<DocInventoryItem>,
    pub total: usize,
    pub stale_count: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DocInventoryItem {
    pub doc_path: String,
    pub doc_type: String,
    pub is_stale: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub staleness_reason: Option<String>,
}
