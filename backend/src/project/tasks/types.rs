// backend/src/project/tasks/types.rs
// Type definitions for project tasks

use serde::{Deserialize, Serialize};

/// Status of a project task
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectTaskStatus {
    Pending,
    InProgress,
    Completed,
    Blocked,
    Cancelled,
}

impl ProjectTaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
            Self::Blocked => "blocked",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "in_progress" => Self::InProgress,
            "completed" => Self::Completed,
            "blocked" => Self::Blocked,
            "cancelled" => Self::Cancelled,
            _ => Self::Pending,
        }
    }
}

/// Priority level for tasks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskPriority {
    Low,
    Medium,
    High,
    Critical,
}

impl TaskPriority {
    pub fn to_int(&self) -> i32 {
        match self {
            Self::Low => 0,
            Self::Medium => 1,
            Self::High => 2,
            Self::Critical => 3,
        }
    }

    pub fn from_int(n: i32) -> Self {
        match n {
            0 => Self::Low,
            1 => Self::Medium,
            2 => Self::High,
            _ => Self::Critical,
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "low" => Self::Low,
            "high" => Self::High,
            "critical" => Self::Critical,
            _ => Self::Medium,
        }
    }
}

/// A persistent project task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectTask {
    pub id: i64,
    pub project_id: String,
    pub parent_task_id: Option<i64>,
    pub user_id: Option<String>,
    pub title: String,
    pub description: Option<String>,
    pub status: ProjectTaskStatus,
    pub priority: i32,
    pub complexity_estimate: Option<f64>,
    pub time_estimate_minutes: Option<i32>,
    pub tags: Vec<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub started_at: Option<i64>,
    pub completed_at: Option<i64>,
}

impl ProjectTask {
    /// Get priority as enum
    pub fn priority_level(&self) -> TaskPriority {
        TaskPriority::from_int(self.priority)
    }
}

/// Input for creating a new task
#[derive(Debug, Clone)]
pub struct NewProjectTask {
    pub project_id: String,
    pub title: String,
    pub description: Option<String>,
    pub priority: TaskPriority,
    pub tags: Vec<String>,
    pub parent_task_id: Option<i64>,
    pub user_id: Option<String>,
}

/// A work session on a task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSession {
    pub id: i64,
    pub task_id: i64,
    pub session_id: String,
    pub user_id: Option<String>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub progress_notes: Option<String>,
    pub files_modified: Vec<String>,
    pub commits: Vec<String>,
}

/// Context attached to a task (artifacts, commits, notes)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskContext {
    pub id: i64,
    pub task_id: i64,
    pub context_type: String,
    pub context_data: String,
    pub created_at: i64,
}

/// Context types for task_context table
pub mod context_types {
    pub const ARTIFACT: &str = "artifact";
    pub const COMMIT: &str = "commit";
    pub const FILE: &str = "file";
    pub const NOTE: &str = "note";
}
