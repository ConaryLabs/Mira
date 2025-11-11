// src/project/types.rs

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub tags: Option<Vec<String>>,
    pub owner: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub id: String,
    pub project_id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub artifact_type: ArtifactType,
    pub content: Option<String>,
    pub version: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactType {
    Code,
    Image,
    Log,
    Note,
    Markdown,
}

impl std::fmt::Display for ArtifactType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ArtifactType::Code => write!(f, "code"),
            ArtifactType::Image => write!(f, "image"),
            ArtifactType::Log => write!(f, "log"),
            ArtifactType::Note => write!(f, "note"),
            ArtifactType::Markdown => write!(f, "markdown"),
        }
    }
}

impl std::str::FromStr for ArtifactType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "code" => Ok(ArtifactType::Code),
            "image" => Ok(ArtifactType::Image),
            "log" => Ok(ArtifactType::Log),
            "note" => Ok(ArtifactType::Note),
            "markdown" => Ok(ArtifactType::Markdown),
            _ => Err(format!("Unknown artifact type: {s}")),
        }
    }
}

// Request/Response types for API

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateProjectRequest {
    pub name: String,
    pub description: Option<String>,
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateProjectRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateArtifactRequest {
    pub project_id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub artifact_type: ArtifactType,
    pub content: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateArtifactRequest {
    pub name: Option<String>,
    pub content: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProjectsResponse {
    pub projects: Vec<Project>,
    pub total: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ArtifactsResponse {
    pub artifacts: Vec<Artifact>,
    pub total: usize,
}
