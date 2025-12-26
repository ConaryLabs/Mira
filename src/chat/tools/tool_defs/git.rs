//! Git tool definitions (read-only for orchestrator)
//! Note: git_commit has been removed - Claude Code handles commits via MCP.

use serde_json::json;
use super::super::definitions::Tool;

pub fn git_tools() -> Vec<Tool> {
    vec![
        Tool {
            tool_type: "function".into(),
            name: "git_status".into(),
            description: Some("Get git repository status: branch, staged, unstaged, and untracked files.".into()),
            parameters: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
        Tool {
            tool_type: "function".into(),
            name: "git_diff".into(),
            description: Some("Show git diff of changes. Use staged=true for staged changes.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "staged": {
                        "type": "boolean",
                        "description": "Show staged changes (default: unstaged)"
                    },
                    "path": {
                        "type": "string",
                        "description": "Limit diff to specific file path"
                    }
                },
                "required": []
            }),
        },
        Tool {
            tool_type: "function".into(),
            name: "git_log".into(),
            description: Some("Show recent git commit history.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "limit": {
                        "type": "integer",
                        "description": "Number of commits to show (default 10)"
                    },
                    "path": {
                        "type": "string",
                        "description": "Show commits for specific file path"
                    }
                },
                "required": []
            }),
        },
        // Git Intelligence Tools
        Tool {
            tool_type: "function".into(),
            name: "get_recent_commits".into(),
            description: Some("Get recent indexed commits with optional filters.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "file_path": {
                        "type": "string",
                        "description": "Filter by file path"
                    },
                    "author": {
                        "type": "string",
                        "description": "Filter by author"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max results (default 20)"
                    }
                },
                "required": []
            }),
        },
        Tool {
            tool_type: "function".into(),
            name: "search_commits".into(),
            description: Some("Search commits by message content.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query for commit messages"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max results (default 20)"
                    }
                },
                "required": ["query"]
            }),
        },
        Tool {
            tool_type: "function".into(),
            name: "find_cochange_patterns".into(),
            description: Some("Find files that frequently change together with a given file.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "file_path": {
                        "type": "string",
                        "description": "File to find co-change patterns for"
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
            name: "find_similar_fixes".into(),
            description: Some("Find past fixes for similar errors. Uses semantic search to match error patterns.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "error": {
                        "type": "string",
                        "description": "Error message to find similar fixes for"
                    },
                    "category": {
                        "type": "string",
                        "description": "Filter by error category"
                    },
                    "language": {
                        "type": "string",
                        "description": "Filter by programming language"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max results (default 5)"
                    }
                },
                "required": ["error"]
            }),
        },
        Tool {
            tool_type: "function".into(),
            name: "record_error_fix".into(),
            description: Some("Record an error fix for future learning. Helps find solutions when similar errors occur.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "error_pattern": {
                        "type": "string",
                        "description": "The error pattern/message"
                    },
                    "fix_description": {
                        "type": "string",
                        "description": "How the error was fixed"
                    },
                    "category": {
                        "type": "string",
                        "description": "Error category: compile, runtime, test, lint"
                    },
                    "language": {
                        "type": "string",
                        "description": "Programming language"
                    },
                    "file_pattern": {
                        "type": "string",
                        "description": "File pattern where this applies"
                    },
                    "fix_diff": {
                        "type": "string",
                        "description": "Diff of the fix"
                    }
                },
                "required": ["error_pattern", "fix_description"]
            }),
        },
    ]
}
