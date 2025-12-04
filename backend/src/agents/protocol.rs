// backend/src/agents/protocol.rs
// IPC protocol for subprocess agents (JSON-based, line-delimited)
//
// Communication flow:
// 1. Mira sends AgentRequest (JSON + newline)
// 2. Agent sends AgentResponse messages (JSON + newline each)
// 3. Agent sends Complete or Error to finish
//
// Tool execution:
// 1. Agent sends ToolRequest
// 2. Mira executes tool and sends ToolResultMessage back
// 3. Agent continues processing

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::agents::types::AgentArtifact;

/// Request sent to a subprocess agent to start execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRequest {
    /// The task to accomplish
    pub task: String,

    /// Additional context for the task
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,

    /// Specific files to examine
    #[serde(default)]
    pub context_files: Vec<String>,

    /// Parent thought signature for reasoning continuity
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thought_signature: Option<String>,

    /// Tools the agent is allowed to use
    #[serde(default)]
    pub allowed_tools: Vec<String>,

    /// Maximum iterations allowed
    #[serde(default = "default_max_iterations")]
    pub max_iterations: u32,

    /// Timeout in milliseconds
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
}

fn default_max_iterations() -> u32 {
    25
}
fn default_timeout() -> u64 {
    300000
}

/// Response messages from a subprocess agent
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentResponse {
    /// Agent wants to call a tool
    ToolRequest {
        /// Unique ID for this tool request
        id: String,
        /// Tool name to call
        name: String,
        /// Tool arguments
        arguments: Value,
    },

    /// Agent is reporting progress
    Progress {
        /// Current iteration number
        iteration: usize,
        /// Maximum iterations allowed
        max_iterations: usize,
        /// Current activity description
        activity: String,
    },

    /// Agent is streaming content
    Streaming {
        /// Content chunk
        content: String,
    },

    /// Agent completed successfully
    Complete {
        /// Final response
        response: String,
        /// Thought signature for continuity
        #[serde(skip_serializing_if = "Option::is_none")]
        thought_signature: Option<String>,
        /// Artifacts produced
        #[serde(default)]
        artifacts: Vec<AgentArtifact>,
    },

    /// Agent encountered an error
    Error {
        /// Error message
        message: String,
    },
}

/// Tool result sent back to subprocess agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultMessage {
    /// Message type (always "tool_result")
    #[serde(rename = "type")]
    pub msg_type: String,

    /// ID matching the ToolRequest
    pub id: String,

    /// Whether the tool succeeded
    pub success: bool,

    /// Tool result (if successful)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,

    /// Error message (if failed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ToolResultMessage {
    pub fn success(id: String, result: Value) -> Self {
        Self {
            msg_type: "tool_result".to_string(),
            id,
            success: true,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: String, error: String) -> Self {
        Self {
            msg_type: "tool_result".to_string(),
            id,
            success: false,
            result: None,
            error: Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_request_serialization() {
        let request = AgentRequest {
            task: "Find all functions".to_string(),
            context: Some("Looking in src/".to_string()),
            context_files: vec!["src/main.rs".to_string()],
            thought_signature: None,
            allowed_tools: vec!["read_project_file".to_string(), "search_codebase".to_string()],
            max_iterations: 10,
            timeout_ms: 60000,
        };

        let json = serde_json::to_string(&request).unwrap();
        let parsed: AgentRequest = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.task, "Find all functions");
        assert_eq!(parsed.allowed_tools.len(), 2);
    }

    #[test]
    fn test_tool_request_response() {
        let response = AgentResponse::ToolRequest {
            id: "call_123".to_string(),
            name: "read_project_file".to_string(),
            arguments: serde_json::json!({"path": "src/main.rs"}),
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("tool_request"));
        assert!(json.contains("read_project_file"));
    }

    #[test]
    fn test_complete_response() {
        let response = AgentResponse::Complete {
            response: "Found 5 functions".to_string(),
            thought_signature: Some("sig_abc".to_string()),
            artifacts: vec![],
        };

        let json = serde_json::to_string(&response).unwrap();
        let parsed: AgentResponse = serde_json::from_str(&json).unwrap();

        match parsed {
            AgentResponse::Complete { response, thought_signature, .. } => {
                assert_eq!(response, "Found 5 functions");
                assert_eq!(thought_signature, Some("sig_abc".to_string()));
            }
            _ => panic!("Wrong response type"),
        }
    }

    #[test]
    fn test_tool_result_message() {
        let success = ToolResultMessage::success(
            "call_123".to_string(),
            serde_json::json!({"content": "file contents"}),
        );
        assert!(success.success);
        assert!(success.result.is_some());

        let error = ToolResultMessage::error(
            "call_456".to_string(),
            "File not found".to_string(),
        );
        assert!(!error.success);
        assert!(error.error.is_some());
    }
}
