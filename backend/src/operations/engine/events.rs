// src/operations/engine/events.rs
// Event types emitted during operation execution

use crate::operations::Artifact;

/// Events emitted by the operation engine
#[derive(Debug, Clone)]
pub enum OperationEngineEvent {
    Started {
        operation_id: String,
    },
    StatusChanged {
        operation_id: String,
        old_status: String,
        new_status: String,
    },
    Streaming {
        operation_id: String,
        content: String,
    },
    Delegated {
        operation_id: String,
        delegated_to: String,
        reason: String,
    },
    ArtifactPreview {
        operation_id: String,
        artifact_id: String,
        path: String,
        preview: String,
    },
    ArtifactCompleted {
        operation_id: String,
        artifact: Artifact,
    },
    Completed {
        operation_id: String,
        result: Option<String>,
        artifacts: Vec<Artifact>,
    },
    Failed {
        operation_id: String,
        error: String,
    },
    /// Sudo command requires user approval
    SudoApprovalRequired {
        operation_id: String,
        approval_request_id: String,
        command: String,
        reason: Option<String>,
    },
    /// Sudo approval was granted
    SudoApproved {
        operation_id: String,
        approval_request_id: String,
        approved_by: String,
    },
    /// Sudo approval was denied
    SudoDenied {
        operation_id: String,
        approval_request_id: String,
        denied_by: String,
        reason: Option<String>,
    },
}
