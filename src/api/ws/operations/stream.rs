// src/api/ws/operations/stream.rs
// UPDATED: event_to_json now includes artifacts in Completed event

use crate::operations::OperationEngineEvent;
use serde_json::Value;

/// Convert OperationEngineEvent to WebSocket JSON format
/// UPDATED: Completed event now includes artifacts array
pub fn event_to_json(event: OperationEngineEvent) -> Value {
    let timestamp = chrono::Utc::now().timestamp();
    
    match event {
        OperationEngineEvent::Started { operation_id } => {
            serde_json::json!({
                "type": "data",
                "dataType": "operation.started",
                "data": {
                    "operation_id": operation_id,
                    "timestamp": timestamp
                }
            })
        }
        OperationEngineEvent::Streaming { operation_id, content } => {
            serde_json::json!({
                "type": "data",
                "dataType": "operation.streaming",
                "data": {
                    "operation_id": operation_id,
                    "content": content,
                    "timestamp": timestamp
                }
            })
        }
        OperationEngineEvent::Delegated { operation_id, delegated_to, reason } => {
            serde_json::json!({
                "type": "data",
                "dataType": "operation.delegated",
                "data": {
                    "operation_id": operation_id,
                    "delegated_to": delegated_to,
                    "reason": reason,
                    "timestamp": timestamp
                }
            })
        }
        OperationEngineEvent::ArtifactPreview { operation_id, artifact_id, path, preview } => {
            serde_json::json!({
                "type": "data",
                "dataType": "operation.artifact_preview",
                "data": {
                    "operation_id": operation_id,
                    "artifact_id": artifact_id,
                    "path": path,
                    "preview": preview,
                    "timestamp": timestamp
                }
            })
        }
        OperationEngineEvent::ArtifactCompleted { operation_id, artifact } => {
            serde_json::json!({
                "type": "data",
                "dataType": "operation.artifact_completed",
                "data": {
                    "operation_id": operation_id,
                    "artifact": {
                        "id": artifact.id,
                        "path": artifact.file_path,
                        "content": artifact.content,
                        "language": artifact.language,
                        "kind": artifact.kind,
                    },
                    "timestamp": timestamp
                }
            })
        }
        // CRITICAL FIX: Completed now includes artifacts array
        OperationEngineEvent::Completed { operation_id, result, artifacts } => {
            serde_json::json!({
                "type": "data",
                "dataType": "operation.completed",
                "data": {
                    "operation_id": operation_id,
                    "result": result,
                    "artifacts": artifacts.iter().map(|artifact| {
                        serde_json::json!({
                            "id": artifact.id,
                            "path": artifact.file_path,
                            "content": artifact.content,
                            "language": artifact.language,
                            "kind": artifact.kind,
                        })
                    }).collect::<Vec<_>>(),
                    "timestamp": timestamp
                }
            })
        }
        OperationEngineEvent::Failed { operation_id, error } => {
            serde_json::json!({
                "type": "data",
                "dataType": "operation.failed",
                "data": {
                    "operation_id": operation_id,
                    "error": error,
                    "timestamp": timestamp
                }
            })
        }
        OperationEngineEvent::StatusChanged { operation_id, old_status, new_status } => {
            serde_json::json!({
                "type": "data",
                "dataType": "operation.status_changed",
                "data": {
                    "operation_id": operation_id,
                    "old_status": old_status,
                    "new_status": new_status,
                    "timestamp": timestamp
                }
            })
        }
    }
}
