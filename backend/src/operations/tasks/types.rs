// src/operations/tasks/types.rs
// Type definitions for operation task tracking

use serde::{Deserialize, Serialize};

/// Status of an operation task
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "TEXT")]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
}

impl TaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskStatus::Pending => "pending",
            TaskStatus::InProgress => "in_progress",
            TaskStatus::Completed => "completed",
            TaskStatus::Failed => "failed",
        }
    }
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// An individual task within an operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationTask {
    pub id: String,
    pub operation_id: String,
    pub sequence: i32,
    pub description: String,
    pub active_form: String,
    pub status: TaskStatus,
    pub created_at: i64,
    pub started_at: Option<i64>,
    pub completed_at: Option<i64>,
    pub error_message: Option<String>,
}

impl OperationTask {
    /// Create a new task
    pub fn new(
        operation_id: String,
        sequence: i32,
        description: String,
        active_form: String,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            operation_id,
            sequence,
            description,
            active_form,
            status: TaskStatus::Pending,
            created_at: chrono::Utc::now().timestamp(),
            started_at: None,
            completed_at: None,
            error_message: None,
        }
    }

    /// Check if task is terminal (completed or failed)
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.status,
            TaskStatus::Completed | TaskStatus::Failed
        )
    }

    /// Check if task is in progress
    pub fn is_in_progress(&self) -> bool {
        self.status == TaskStatus::InProgress
    }
}
