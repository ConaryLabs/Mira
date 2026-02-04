use schemars::JsonSchema;
use serde::Serialize;

use super::ToolOutput;

pub type ProjectOutput = ToolOutput<ProjectData>;

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum ProjectData {
    Start(ProjectStartData),
    Get(ProjectGetData),
    Set(ProjectSetData),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ProjectStartData {
    pub project_id: i64,
    pub project_name: Option<String>,
    pub project_path: String,
    pub project_type: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ProjectGetData {
    pub project_id: i64,
    pub project_name: Option<String>,
    pub project_path: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ProjectSetData {
    pub project_id: i64,
    pub project_name: Option<String>,
}
