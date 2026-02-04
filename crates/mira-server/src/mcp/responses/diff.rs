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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub historical_risk: Option<HistoricalRiskData>,
}

/// Historical risk data for structured output
#[derive(Debug, Serialize, JsonSchema)]
pub struct HistoricalRiskData {
    /// Risk adjustment: "elevated" or "normal"
    pub risk_delta: String,
    /// Patterns that matched this diff
    pub matching_patterns: Vec<PatternMatchInfo>,
    /// Weighted average confidence
    pub overall_confidence: f64,
}

/// A single matched pattern in structured output
#[derive(Debug, Serialize, JsonSchema)]
pub struct PatternMatchInfo {
    /// Pattern subtype: "module_hotspot", "co_change_gap", "size_risk"
    pub pattern_type: String,
    /// Human-readable description
    pub description: String,
    /// Pattern confidence (0.0-1.0)
    pub confidence: f64,
}
