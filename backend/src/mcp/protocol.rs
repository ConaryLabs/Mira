// backend/src/mcp/protocol.rs
// JSON-RPC 2.0 protocol types for MCP communication

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// JSON-RPC 2.0 Request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JsonRpcRequest {
    pub fn new(id: i64, method: &str, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: Some(id),
            method: method.to_string(),
            params,
        }
    }

    /// Create a notification (no id, no response expected)
    pub fn notification(method: &str, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: None,
            method: method.to_string(),
            params,
        }
    }
}

/// JSON-RPC 2.0 Error
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// JSON-RPC 2.0 Response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

/// MCP Server Capabilities
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpCapabilities {
    #[serde(default)]
    pub tools: Option<ToolCapabilities>,
    #[serde(default)]
    pub resources: Option<ResourceCapabilities>,
    #[serde(default)]
    pub prompts: Option<PromptCapabilities>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolCapabilities {
    #[serde(default)]
    pub list_changed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ResourceCapabilities {
    #[serde(default)]
    pub subscribe: bool,
    #[serde(default)]
    pub list_changed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PromptCapabilities {
    #[serde(default)]
    pub list_changed: bool,
}

/// MCP Tool definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub input_schema: Option<Value>,
}

impl McpTool {
    /// Convert to OpenAI-compatible tool format for LLM
    pub fn to_openai_format(&self, server_name: &str) -> Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": format!("mcp__{}__{}", server_name, self.name),
                "description": self.description.clone().unwrap_or_else(|| format!("MCP tool: {}", self.name)),
                "parameters": self.input_schema.clone().unwrap_or(serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }))
            }
        })
    }
}

/// MCP Resource definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResource {
    pub uri: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub mime_type: Option<String>,
}

/// MCP Prompt definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpPrompt {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub arguments: Vec<McpPromptArgument>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpPromptArgument {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub required: bool,
}

/// Tool call result content
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultContent {
    #[serde(rename = "type")]
    pub content_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

/// Tool call result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    #[serde(default)]
    pub content: Vec<ToolResultContent>,
    #[serde(default)]
    pub is_error: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_rpc_request() {
        let req = JsonRpcRequest::new(1, "test", Some(serde_json::json!({"key": "value"})));
        assert_eq!(req.jsonrpc, "2.0");
        assert_eq!(req.id, Some(1));
        assert_eq!(req.method, "test");
    }

    #[test]
    fn test_notification() {
        let notif = JsonRpcRequest::notification("notify", None);
        assert_eq!(notif.id, None);
    }

    #[test]
    fn test_mcp_tool_to_openai() {
        let tool = McpTool {
            name: "read_file".to_string(),
            description: Some("Read a file".to_string()),
            input_schema: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"}
                },
                "required": ["path"]
            })),
        };

        let openai = tool.to_openai_format("filesystem");
        assert_eq!(
            openai["function"]["name"].as_str().unwrap(),
            "mcp__filesystem__read_file"
        );
    }

    #[test]
    fn test_parse_json_rpc_response() {
        let json = r#"{"jsonrpc":"2.0","id":1,"result":{"success":true}}"#;
        let resp: JsonRpcResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.id, Some(1));
        assert!(resp.result.is_some());
        assert!(resp.error.is_none());
    }

    #[test]
    fn test_parse_json_rpc_error() {
        let json = r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32600,"message":"Invalid Request"}}"#;
        let resp: JsonRpcResponse = serde_json::from_str(json).unwrap();
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32600);
    }
}
