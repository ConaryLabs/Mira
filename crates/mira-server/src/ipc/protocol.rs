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
}
