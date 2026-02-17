use schemars::JsonSchema;
use serde::Serialize;

use super::ToolOutput;

pub type SessionOutput = ToolOutput<SessionData>;

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum SessionData {
    Current(SessionCurrentData),
    ListSessions(SessionListData),
    History(SessionHistoryData),
    Insights(InsightsData),
    ErrorPatterns(ErrorPatternsData),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SessionCurrentData {
    pub session_id: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SessionListData {
    pub sessions: Vec<SessionSummary>,
    pub total: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SessionSummary {
    pub id: String,
    pub started_at: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resumed_from: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SessionHistoryData {
    pub session_id: String,
    pub entries: Vec<HistoryEntry>,
    pub total: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct HistoryEntry {
    pub tool_name: String,
    pub created_at: String,
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_preview: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct InsightsData {
    pub insights: Vec<InsightItem>,
    pub total: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct InsightItem {
    /// Row ID for dismissable insights (pondering, doc_gap). Use with dismiss_insight action.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub row_id: Option<i64>,
    pub source: String,
    pub source_type: String,
    pub description: String,
    pub priority_score: f64,
    pub confidence: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
    /// Trend direction when applicable (health insights)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trend: Option<String>,
    /// Change summary when applicable (health insights)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change_summary: Option<String>,
    /// Dashboard category for grouping
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ErrorPatternsData {
    pub patterns: Vec<ErrorPatternItem>,
    pub total: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ErrorPatternItem {
    pub tool_name: String,
    pub error_fingerprint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix_description: Option<String>,
    pub occurrence_count: i64,
    pub last_seen: String,
}
