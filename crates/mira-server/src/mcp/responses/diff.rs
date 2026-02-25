use schemars::JsonSchema;
use serde::Serialize;

use super::ToolOutput;

pub type DiffOutput = ToolOutput<DiffData>;

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum DiffData {
    Analysis(DiffAnalysisData),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DiffAnalysisData {
    pub from_ref: String,
    pub to_ref: String,
    pub files_changed: i64,
    pub lines_added: i64,
    pub lines_removed: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk_level: Option<String>,
    /// Always None — historical risk pipeline has been removed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub historical_risk: Option<serde_json::Value>,
}
