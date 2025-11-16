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
    }
}
