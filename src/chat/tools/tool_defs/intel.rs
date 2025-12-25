//! Code intelligence, build, document, and index tool definitions

use serde_json::json;
use super::super::definitions::Tool;

pub fn intel_tools() -> Vec<Tool> {
    vec![
        // Code Intelligence Tools
        Tool {
            tool_type: "function".into(),
            name: "get_symbols".into(),
            description: Some("Get code symbols (functions, classes, methods) from a file.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "file_path": {
                        "type": "string",
                        "description": "Path to the file to analyze"
                    },
                    "symbol_type": {
                        "type": "string",
                        "description": "Filter by type: function, class, method, struct, enum"
                    }
                },
                "required": ["file_path"]
            }),
        },
        Tool {
            tool_type: "function".into(),
            name: "get_call_graph".into(),
            description: Some("Get call graph for a function - what calls it and what it calls.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "symbol": {
                        "type": "string",
                        "description": "Name of the function/method to analyze"
                    },
                    "depth": {
                        "type": "integer",
                        "description": "Depth of call graph traversal (default 2)"
                    }
                },
                "required": ["symbol"]
            }),
        },
        Tool {
            tool_type: "function".into(),
            name: "semantic_code_search".into(),
            description: Some("Search code by meaning using natural language. Finds similar code patterns and implementations.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Natural language description of code to find"
                    },
                    "language": {
                        "type": "string",
                        "description": "Filter by programming language"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max results (default 10)"
                    }
                },
                "required": ["query"]
            }),
        },
        Tool {
            tool_type: "function".into(),
            name: "get_related_files".into(),
            description: Some("Find files related to a given file by imports or co-change patterns.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "file_path": {
                        "type": "string",
                        "description": "Path to the file to find related files for"
                    },
                    "relation_type": {
                        "type": "string",
                        "description": "Type: imports, cochange, or both (default)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max results (default 10)"
                    }
                },
                "required": ["file_path"]
            }),
        },
        Tool {
            tool_type: "function".into(),
            name: "get_codebase_style".into(),
            description: Some("Analyze codebase style metrics: function lengths, complexity, patterns.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "project_path": {
                        "type": "string",
                        "description": "Project path to analyze (default: current directory)"
                    }
                },
                "required": []
            }),
        },
        // Build Tracking Tools
        Tool {
            tool_type: "function".into(),
            name: "build".into(),
            description: Some("Track build runs and errors. Actions: record, record_error, get_errors, resolve.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "description": "Action: record/record_error/get_errors/resolve"
                    },
                    "command": {
                        "type": "string",
                        "description": "Build command (for record)"
                    },
                    "success": {
                        "type": "boolean",
                        "description": "Whether build succeeded (for record)"
                    },
                    "duration_ms": {
                        "type": "integer",
                        "description": "Build duration in milliseconds"
                    },
                    "message": {
                        "type": "string",
                        "description": "Error message (for record_error)"
                    },
                    "category": {
                        "type": "string",
                        "description": "Error category"
                    },
                    "severity": {
                        "type": "string",
                        "description": "Error severity: error/warning"
                    },
                    "file_path": {
                        "type": "string",
                        "description": "File path for error"
                    },
                    "line_number": {
                        "type": "integer",
                        "description": "Line number for error"
                    },
                    "error_id": {
                        "type": "integer",
                        "description": "Error ID (for resolve)"
                    },
                    "include_resolved": {
                        "type": "boolean",
                        "description": "Include resolved errors (for get_errors)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max results (for get_errors)"
                    }
                },
                "required": ["action"]
            }),
        },
        // Document Tools
        Tool {
            tool_type: "function".into(),
            name: "document".into(),
            description: Some("Manage indexed documents. Actions: list, search, get, delete.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "description": "Action: list/search/get/delete"
                    },
                    "document_id": {
                        "type": "string",
                        "description": "Document ID (for get/delete)"
                    },
                    "query": {
                        "type": "string",
                        "description": "Search query (for search)"
                    },
                    "doc_type": {
                        "type": "string",
                        "description": "Filter by type: pdf/markdown/text/code"
                    },
                    "include_content": {
                        "type": "boolean",
                        "description": "Include full content (for get)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max results"
                    }
                },
                "required": ["action"]
            }),
        },
        // Index Tools
        Tool {
            tool_type: "function".into(),
            name: "index".into(),
            description: Some("Index code and git history. Actions: project, file, status, cleanup.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "description": "Action: project/file/status/cleanup"
                    },
                    "path": {
                        "type": "string",
                        "description": "Path to index (for project/file)"
                    },
                    "include_git": {
                        "type": "boolean",
                        "description": "Include git history (for project, default true)"
                    },
                    "commit_limit": {
                        "type": "integer",
                        "description": "Max commits to index (default 500)"
                    },
                    "parallel": {
                        "type": "boolean",
                        "description": "Use parallel processing (default true)"
                    }
                },
                "required": ["action"]
            }),
        },
        // Proactive Context Tool
        Tool {
            tool_type: "function".into(),
            name: "get_proactive_context".into(),
            description: Some("Get all relevant context for the current work: corrections, goals, rejected approaches, similar errors, code relationships.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "files": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Files you're working with"
                    },
                    "topics": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Topics/concepts you're working on"
                    },
                    "task": {
                        "type": "string",
                        "description": "Current task description"
                    },
                    "error": {
                        "type": "string",
                        "description": "Error message if debugging"
                    },
                    "limit_per_category": {
                        "type": "integer",
                        "description": "Max items per category (default 3)"
                    }
                },
                "required": []
            }),
        },
    ]
}
