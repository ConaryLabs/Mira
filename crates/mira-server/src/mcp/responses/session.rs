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
    pub source: String,
    pub source_type: String,
    pub description: String,
    pub priority_score: f64,
    pub confidence: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
}
