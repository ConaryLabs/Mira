use schemars::JsonSchema;
use serde::Serialize;

use super::ToolOutput;

pub type TeamOutput = ToolOutput<TeamData>;

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum TeamData {
    Status(TeamStatusData),
    Review(TeamReviewData),
    Distill(TeamDistillData),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TeamStatusData {
    pub team_name: String,
    pub team_id: i64,
    pub members: Vec<TeamMemberSummary>,
    pub active_count: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub file_conflicts: Vec<FileConflictSummary>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TeamMemberSummary {
    pub name: String,
    pub role: String,
    pub status: String,
    pub last_heartbeat: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct FileConflictSummary {
    pub file_path: String,
    pub edited_by: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TeamReviewData {
    pub member_name: String,
    pub files_modified: Vec<String>,
    pub file_count: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TeamDistillData {
    pub team_name: String,
    pub findings_count: usize,
    pub memories_processed: usize,
    pub files_touched: usize,
    pub findings: Vec<DistilledFindingSummary>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DistilledFindingSummary {
    pub category: String,
    pub content: String,
    pub source_count: usize,
}
