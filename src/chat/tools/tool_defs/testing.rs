//! Artifact tool definitions
//! Note: run_tests has been removed - Claude Code runs tests via MCP.

use serde_json::json;
use super::super::definitions::Tool;

pub fn artifact_tools() -> Vec<Tool> {
    vec![
        Tool {
            tool_type: "function".into(),
            name: "fetch_artifact".into(),
            description: Some("Fetch a slice of a stored artifact. Use when tool output was truncated and you need more content.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "artifact_id": {
                        "type": "string",
                        "description": "The artifact ID from the truncated tool output"
                    },
                    "offset": {
                        "type": "integer",
                        "description": "Character offset to start from (default 0)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max characters to fetch (default 8192, max 16384)"
                    }
                },
                "required": ["artifact_id"]
            }),
        },
        Tool {
            tool_type: "function".into(),
            name: "search_artifact".into(),
            description: Some("Search within a stored artifact. Use to find specific content in large tool outputs.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "artifact_id": {
                        "type": "string",
                        "description": "The artifact ID to search within"
                    },
                    "query": {
                        "type": "string",
                        "description": "Text to search for (case-insensitive)"
                    },
                    "max_results": {
                        "type": "integer",
                        "description": "Max matches to return (default 5, max 20)"
                    },
                    "context_bytes": {
                        "type": "integer",
                        "description": "Bytes of context around each match (default 200)"
                    }
                },
                "required": ["artifact_id", "query"]
            }),
        },
    ]
}
