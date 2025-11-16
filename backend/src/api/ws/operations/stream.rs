// src/api/ws/operations/stream.rs
// Convert OperationEngineEvent to WebSocket JSON format

use crate::operations::OperationEngineEvent;
use serde_json::Value;

/// Convert OperationEngineEvent to WebSocket JSON format
pub fn event_to_json(event: OperationEngineEvent) -> Value {
    let timestamp = chrono::Utc::now().timestamp();

    match event {
        OperationEngineEvent::Started { operation_id } => {
            serde_json::json!({
                "type": "operation.started",
                "operation_id": operation_id,
                "timestamp": timestamp
            })
        }
        OperationEngineEvent::Streaming {
            operation_id,
            content,
        } => {
            serde_json::json!({
                "type": "operation.streaming",
                "operation_id": operation_id,
                "content": content,
                "timestamp": timestamp
            })
        }
        OperationEngineEvent::PlanGenerated {
            operation_id,
            plan_text,
            reasoning_tokens,
        } => {
            serde_json::json!({
                "type": "operation.plan_generated",
                "operation_id": operation_id,
                "plan_text": plan_text,
                "reasoning_tokens": reasoning_tokens,
                "timestamp": timestamp
            })
        }
        OperationEngineEvent::Delegated {
            operation_id,
            delegated_to,
            reason,
        } => {
            serde_json::json!({
                "type": "operation.delegated",
                "operation_id": operation_id,
                "delegated_to": delegated_to,
                "reason": reason,
                "timestamp": timestamp
            })
        }
        OperationEngineEvent::ArtifactPreview {
            operation_id,
            artifact_id,
            path,
            preview,
        } => {
            serde_json::json!({
                "type": "operation.artifact_preview",
                "operation_id": operation_id,
                "artifact_id": artifact_id,
                "path": path,
                "preview": preview,
                "timestamp": timestamp
            })
        }
        OperationEngineEvent::ArtifactCompleted {
            operation_id,
            artifact,
        } => {
            serde_json::json!({
                "type": "operation.artifact_completed",
                "operation_id": operation_id,
                "artifact": {
                    "id": artifact.id,
                    "path": artifact.file_path,
                    "content": artifact.content,
                    "language": artifact.language,
                    "kind": artifact.kind,
                },
                "timestamp": timestamp
            })
        }
        OperationEngineEvent::Completed {
            operation_id,
            result,
            artifacts,
        } => {
            // Serialize artifacts into JSON format for the frontend
            let artifacts_json: Vec<Value> = artifacts
                .into_iter()
                .map(|artifact| {
                    serde_json::json!({
                        "id": artifact.id,
                        "path": artifact.file_path,
                        "content": artifact.content,
                        "language": artifact.language,
                        "kind": artifact.kind,
                    })
                })
                .collect();

            serde_json::json!({
                "type": "operation.completed",
                "operation_id": operation_id,
                "result": result,
                "artifacts": artifacts_json,
                "timestamp": timestamp
            })
        }
        OperationEngineEvent::Failed {
            operation_id,
            error,
        } => {
            serde_json::json!({
                "type": "operation.failed",
                "operation_id": operation_id,
                "error": error,
                "timestamp": timestamp
            })
        }
        OperationEngineEvent::TaskCreated {
            operation_id,
            task_id,
            sequence,
            description,
            active_form,
        } => {
            serde_json::json!({
                "type": "operation.task_created",
                "operation_id": operation_id,
                "task_id": task_id,
                "sequence": sequence,
                "description": description,
                "active_form": active_form,
                "timestamp": timestamp
            })
        }
        OperationEngineEvent::TaskStarted {
            operation_id,
            task_id,
        } => {
            serde_json::json!({
                "type": "operation.task_started",
                "operation_id": operation_id,
                "task_id": task_id,
                "timestamp": timestamp
            })
        }
        OperationEngineEvent::TaskCompleted {
            operation_id,
            task_id,
        } => {
            serde_json::json!({
                "type": "operation.task_completed",
                "operation_id": operation_id,
                "task_id": task_id,
                "timestamp": timestamp
            })
        }
        OperationEngineEvent::TaskFailed {
            operation_id,
            task_id,
            error,
        } => {
            serde_json::json!({
                "type": "operation.task_failed",
                "operation_id": operation_id,
                "task_id": task_id,
                "error": error,
                "timestamp": timestamp
            })
        }
        OperationEngineEvent::StatusChanged {
            operation_id,
            old_status,
            new_status,
        } => {
            serde_json::json!({
                "type": "operation.status_changed",
                "operation_id": operation_id,
                "old_status": old_status,
                "new_status": new_status,
                "timestamp": timestamp
            })
        }
        OperationEngineEvent::SudoApprovalRequired {
            operation_id,
            approval_request_id,
            command,
            reason,
        } => {
            serde_json::json!({
                "type": "operation.sudo_approval_required",
                "operation_id": operation_id,
                "approval_request_id": approval_request_id,
                "command": command,
                "reason": reason,
                "timestamp": timestamp
            })
        }
        OperationEngineEvent::SudoApproved {
            operation_id,
            approval_request_id,
            approved_by,
        } => {
            serde_json::json!({
                "type": "operation.sudo_approved",
                "operation_id": operation_id,
                "approval_request_id": approval_request_id,
                "approved_by": approved_by,
                "timestamp": timestamp
            })
        }
        OperationEngineEvent::SudoDenied {
            operation_id,
            approval_request_id,
            denied_by,
            reason,
        } => {
            serde_json::json!({
                "type": "operation.sudo_denied",
                "operation_id": operation_id,
                "approval_request_id": approval_request_id,
                "denied_by": denied_by,
                "reason": reason,
                "timestamp": timestamp
            })
        }
        OperationEngineEvent::ToolExecuted {
            operation_id,
            tool_name,
            tool_type,
            summary,
            success,
            details,
        } => {
            serde_json::json!({
                "type": "operation.tool_executed",
                "operation_id": operation_id,
                "tool_name": tool_name,
                "tool_type": tool_type,
                "summary": summary,
                "success": success,
                "details": details,
                "timestamp": timestamp
            })
        }
    }
}
