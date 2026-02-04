use schemars::JsonSchema;
use serde::Serialize;

use super::ToolOutput;

pub type FindingOutput = ToolOutput<FindingData>;

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum FindingData {
    List(FindingListData),
    Get(Box<FindingGetData>),
    Stats(FindingStatsData),
    Patterns(FindingPatternsData),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct FindingListData {
    pub findings: Vec<FindingItem>,
    pub stats: FindingStatsData,
    pub total: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct FindingItem {
    pub id: i64,
    pub finding_type: String,
    pub severity: String,
    pub status: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feedback: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct FindingGetData {
    pub id: i64,
    pub finding_type: String,
    pub severity: String,
    pub status: String,
    pub expert_role: String,
    pub confidence: f64,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_snippet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feedback: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reviewed_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reviewed_at: Option<String>,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FindingStatsData {
    pub pending: i64,
    pub accepted: i64,
    pub rejected: i64,
    pub fixed: i64,
    pub total: i64,
    pub acceptance_rate: f64,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct FindingPatternsData {
    pub patterns: Vec<LearnedPattern>,
    pub total: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct LearnedPattern {
    pub id: i64,
    pub correction_type: String,
    pub confidence: f64,
    pub occurrence_count: i64,
    pub acceptance_rate: f64,
    pub what_was_wrong: String,
    pub what_is_right: String,
}
