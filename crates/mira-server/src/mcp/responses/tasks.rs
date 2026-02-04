use schemars::JsonSchema;
use serde::Serialize;

use super::ToolOutput;

pub type TasksOutput = ToolOutput<TasksData>;

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum TasksData {
    List(TasksListData),
    Status(TasksStatusData),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TasksListData {
    pub tasks: Vec<TaskSummary>,
    pub total: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TaskSummary {
    pub task_id: String,
    pub tool_name: String,
    pub status: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TasksStatusData {
    pub task_id: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_structured: Option<serde_json::Value>,
}
