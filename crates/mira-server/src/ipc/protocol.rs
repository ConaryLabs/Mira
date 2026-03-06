// crates/mira-server/src/ipc/protocol.rs
// NDJSON protocol types for hook-to-server IPC

use serde::{Deserialize, Serialize};

/// IPC request sent by hooks over the Unix socket.
///
/// Line-delimited JSON: one request line → one response line → close.
#[derive(Debug, Serialize, Deserialize)]
pub struct IpcRequest {
    /// Operation name (e.g. "resolve_project", "recall_memories")
    pub op: String,
    /// Request ID for correlation (UUID)
    pub id: String,
    /// Operation-specific parameters
    #[serde(default)]
    pub params: serde_json::Value,
}

/// IPC response sent by the server back to the hook.
#[derive(Debug, Serialize, Deserialize)]
pub struct IpcResponse {
    /// Correlation ID matching the request
    pub id: String,
    /// Whether the operation succeeded
    pub ok: bool,
    /// Operation result (present when ok=true)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    /// Error message (present when ok=false)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl IpcResponse {
    /// Create a success response with the given result.
    pub fn success(id: String, result: serde_json::Value) -> Self {
        Self {
            id,
            ok: true,
            result: Some(result),
            error: None,
        }
    }

    /// Create an error response with the given message.
    pub fn error(id: String, message: impl Into<String>) -> Self {
        Self {
            id,
            ok: false,
            result: None,
            error: Some(message.into()),
        }
    }
}

/// Server-pushed event over a persistent subscription connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcPushEvent {
    pub event_type: String,
    pub sequence: u64,
    pub data: serde_json::Value,
}

/// Full state snapshot sent on initial subscribe.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStateSnapshot {
    pub sequence: u64,
    pub goals: Vec<GoalSnapshot>,
    pub injection_stats: InjectionStatsSnapshot,
    pub modified_files: Vec<String>,
    pub team_conflicts: Vec<FileConflictSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalSnapshot {
    pub id: i64,
    pub title: String,
    pub status: String,
    pub progress_percent: i32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InjectionStatsSnapshot {
    pub total_injections: u64,
    pub total_chars: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileConflictSnapshot {
    pub file_path: String,
    pub other_member_name: String,
    pub operation: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_roundtrip() {
        let req = IpcRequest {
            op: "recall_memories".into(),
            id: "test-123".into(),
            params: serde_json::json!({"project_id": 42, "query": "auth"}),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: IpcRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.op, "recall_memories");
        assert_eq!(parsed.id, "test-123");
        assert_eq!(parsed.params["project_id"], 42);
    }

    #[test]
    fn success_response_serialization() {
        let resp = IpcResponse::success("id-1".into(), serde_json::json!({"memories": ["a"]}));
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"ok\":true"));
        assert!(!json.contains("\"error\""));
    }

    #[test]
    fn error_response_serialization() {
        let resp = IpcResponse::error("id-2".into(), "not found");
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"ok\":false"));
        assert!(json.contains("not found"));
        assert!(!json.contains("\"result\""));
    }

    #[test]
    fn request_with_empty_params() {
        let json = r#"{"op":"resolve_project","id":"x","params":{}}"#;
        let req: IpcRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.op, "resolve_project");
        assert!(req.params.is_object());
    }

    #[test]
    fn request_without_params_field() {
        let json = r#"{"op":"resolve_project","id":"x"}"#;
        let req: IpcRequest = serde_json::from_str(json).unwrap();
        assert!(req.params.is_null());
    }

    #[test]
    fn push_event_roundtrip() {
        let event = IpcPushEvent {
            event_type: "goal_updated".to_string(),
            sequence: 1,
            data: serde_json::json!({ "goal_id": 5, "progress": 80 }),
        };
        let json = serde_json::to_string(&event).unwrap();
        let parsed: IpcPushEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.event_type, "goal_updated");
        assert_eq!(parsed.sequence, 1);
        assert_eq!(parsed.data["goal_id"], 5);
    }

    #[test]
    fn session_state_snapshot_roundtrip() {
        let snapshot = SessionStateSnapshot {
            sequence: 0,
            goals: vec![GoalSnapshot {
                id: 1,
                title: "Test goal".to_string(),
                status: "in_progress".to_string(),
                progress_percent: 50,
            }],
            injection_stats: InjectionStatsSnapshot { total_injections: 5, total_chars: 1200 },
            modified_files: vec!["src/main.rs".to_string()],
            team_conflicts: vec![],
        };
        let json = serde_json::to_string(&snapshot).unwrap();
        let parsed: SessionStateSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.goals.len(), 1);
        assert_eq!(parsed.goals[0].progress_percent, 50);
        assert_eq!(parsed.injection_stats.total_injections, 5);
        assert_eq!(parsed.modified_files, vec!["src/main.rs"]);
    }

    #[test]
    fn injection_stats_snapshot_default() {
        let stats = InjectionStatsSnapshot::default();
        assert_eq!(stats.total_injections, 0);
        assert_eq!(stats.total_chars, 0);
    }
}
