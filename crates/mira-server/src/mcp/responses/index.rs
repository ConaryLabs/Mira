use schemars::JsonSchema;
use serde::Serialize;

use super::ToolOutput;

pub type IndexOutput = ToolOutput<IndexData>;

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum IndexData {
    Project(IndexProjectData),
    Status(IndexStatusData),
    Compact(IndexCompactData),
    Summarize(IndexSummarizeData),
    Health(IndexHealthData),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IndexProjectData {
    pub files: usize,
    pub symbols: usize,
    pub chunks: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modules_summarized: Option<usize>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IndexStatusData {
    pub symbols: usize,
    pub embedded_chunks: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IndexCompactData {
    pub rows_preserved: usize,
    pub estimated_savings_mb: f64,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IndexSummarizeData {
    pub modules_summarized: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IndexHealthData {
    pub issues_found: usize,
}
