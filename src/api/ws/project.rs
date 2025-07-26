// src/api/ws/project.rs

use serde::{Deserialize, Serialize};

// We'll define minimal types here to avoid circular dependencies
// The actual Project and Artifact types should be imported from the project module

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum WsProjectClientMessage {
    #[serde(rename = "switch_project")]
    SwitchProject {
        project_id: String,
    },
    #[serde(rename = "create_project")]
    CreateProject {
        name: String,
        description: Option<String>,
        tags: Option<Vec<String>>,
    },
    #[serde(rename = "update_project")]
    UpdateProject {
        project_id: String,
        name: Option<String>,
        description: Option<String>,
        tags: Option<Vec<String>>,
    },
    #[serde(rename = "delete_project")]
    DeleteProject {
        project_id: String,
    },
    #[serde(rename = "save_as_artifact")]
    SaveAsArtifact {
        message_id: String,
        project_id: String,
        name: String,
        artifact_type: String,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum WsProjectServerMessage {
    #[serde(rename = "project_switched")]
    ProjectSwitched {
        project_id: String,
        project_name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        artifacts_count: Option<usize>,
    },
    #[serde(rename = "project_created")]
    ProjectCreated {
        project_id: String,
        project_name: String,
    },
    #[serde(rename = "project_updated")]
    ProjectUpdated {
        project_id: String,
        project_name: String,
    },
    #[serde(rename = "project_deleted")]
    ProjectDeleted {
        project_id: String,
    },
    #[serde(rename = "artifact_created")]
    ArtifactCreated {
        artifact_id: String,
        artifact_name: String,
        project_id: String,
    },
    #[serde(rename = "artifact_updated")]
    ArtifactUpdated {
        artifact_id: String,
        artifact_name: String,
    },
    #[serde(rename = "artifact_deleted")]
    ArtifactDeleted {
        artifact_id: String,
    },
    #[serde(rename = "project_list")]
    ProjectList {
        projects: Vec<ProjectInfo>,
        active_project_id: Option<String>,
    },
    #[serde(rename = "artifact_list")]
    ArtifactList {
        project_id: String,
        artifacts: Vec<ArtifactInfo>,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProjectInfo {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub artifacts_count: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ArtifactInfo {
    pub id: String,
    pub name: String,
    pub artifact_type: String,
    pub created_at: String,
}
