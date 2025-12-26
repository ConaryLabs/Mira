//! File operation tool definitions (read-only for orchestrator)
//! Note: write_file, edit_file, and bash have been removed.
//! Claude Code handles all file modifications via MCP.

use serde_json::json;
use super::super::definitions::Tool;

pub fn file_ops_tools() -> Vec<Tool> {
    vec![
        Tool {
            tool_type: "function".into(),
            name: "read_file".into(),
            description: Some("Read the contents of a file. For large files (>1MB), use offset/limit to read portions.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file to read"
                    },
                    "offset": {
                        "type": "integer",
                        "description": "Line number to start reading from (0-indexed)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of lines to read"
                    }
                },
                "required": ["path"]
            }),
        },
        Tool {
            tool_type: "function".into(),
            name: "glob".into(),
            description: Some("Find files matching a glob pattern".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "Glob pattern (e.g., **/*.rs)"
                    },
                    "path": {
                        "type": "string",
                        "description": "Base directory to search from"
                    }
                },
                "required": ["pattern"]
            }),
        },
        Tool {
            tool_type: "function".into(),
            name: "grep".into(),
            description: Some("Search for a pattern in files".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "Regex pattern to search for"
                    },
                    "path": {
                        "type": "string",
                        "description": "Directory to search in"
                    }
                },
                "required": ["pattern"]
            }),
        },
    ]
}
