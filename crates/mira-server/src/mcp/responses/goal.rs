use schemars::JsonSchema;
use serde::Serialize;

use super::ToolOutput;

pub type GoalOutput = ToolOutput<GoalData>;

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum GoalData {
    Created(GoalCreatedData),
    BulkCreated(GoalBulkCreatedData),
    List(GoalListData),
    Get(GoalGetData),
    MilestoneProgress(MilestoneProgressData),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct GoalCreatedData {
    pub goal_id: i64,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct GoalBulkCreatedData {
    pub goals: Vec<GoalCreatedEntry>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct GoalCreatedEntry {
    pub id: i64,
    pub title: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct GoalListData {
    pub goals: Vec<GoalSummary>,
    pub total: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct GoalSummary {
    pub id: i64,
    pub title: String,
    pub status: String,
    pub priority: String,
    pub progress_percent: i32,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct GoalGetData {
    pub id: i64,
    pub title: String,
    pub status: String,
    pub priority: String,
    pub progress_percent: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub created_at: String,
    pub milestones: Vec<MilestoneInfo>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct MilestoneInfo {
    pub id: i64,
    pub title: String,
    pub weight: i32,
    pub completed: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct MilestoneProgressData {
    pub milestone_id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub goal_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress_percent: Option<i32>,
}
