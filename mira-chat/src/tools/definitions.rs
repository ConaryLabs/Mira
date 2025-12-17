//! Tool definitions for GPT-5.2 function calling

use serde_json::json;

use crate::responses::Tool;

/// Get all tool definitions for GPT-5.2
pub fn get_tools() -> Vec<Tool> {
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
            name: "write_file".into(),
            description: Some("Write content to a file".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to write to"
                    },
                    "content": {
                        "type": "string",
                        "description": "Content to write"
                    }
                },
                "required": ["path", "content"]
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
        Tool {
            tool_type: "function".into(),
            name: "bash".into(),
            description: Some("Execute a shell command".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "Shell command to execute"
                    }
                },
                "required": ["command"]
            }),
        },
        Tool {
            tool_type: "function".into(),
            name: "edit_file".into(),
            description: Some("Edit a file by replacing old_string with new_string. The old_string must match exactly and be unique in the file.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file to edit"
                    },
                    "old_string": {
                        "type": "string",
                        "description": "The exact text to find and replace"
                    },
                    "new_string": {
                        "type": "string",
                        "description": "The text to replace old_string with"
                    },
                    "replace_all": {
                        "type": "boolean",
                        "description": "If true, replace all occurrences. Default false."
                    }
                },
                "required": ["path", "old_string", "new_string"]
            }),
        },
        Tool {
            tool_type: "function".into(),
            name: "web_search".into(),
            description: Some("Search the web using DuckDuckGo".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max number of results (default 5)"
                    }
                },
                "required": ["query"]
            }),
        },
        Tool {
            tool_type: "function".into(),
            name: "web_fetch".into(),
            description: Some("Fetch content from a URL and convert to text".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "URL to fetch"
                    },
                    "max_length": {
                        "type": "integer",
                        "description": "Max content length (default 10000)"
                    }
                },
                "required": ["url"]
            }),
        },
        Tool {
            tool_type: "function".into(),
            name: "remember".into(),
            description: Some("Store a fact, decision, or preference for future recall. Uses semantic search for intelligent retrieval.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "content": {
                        "type": "string",
                        "description": "The fact or information to remember"
                    },
                    "fact_type": {
                        "type": "string",
                        "description": "Type of fact: preference, decision, context, general (default)"
                    },
                    "category": {
                        "type": "string",
                        "description": "Optional category for organization"
                    }
                },
                "required": ["content"]
            }),
        },
        Tool {
            tool_type: "function".into(),
            name: "recall".into(),
            description: Some("Search for previously stored memories using semantic similarity. Returns relevant facts, decisions, and preferences.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query - uses semantic similarity matching"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max number of results (default 5)"
                    },
                    "fact_type": {
                        "type": "string",
                        "description": "Filter by fact type: preference, decision, context, general"
                    }
                },
                "required": ["query"]
            }),
        },
        // ================================================================
        // Mira Power Armor Tools
        // ================================================================
        Tool {
            tool_type: "function".into(),
            name: "task".into(),
            description: Some("Manage persistent tasks. Actions: create, list, update, complete, delete.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "description": "Action: create/list/update/complete/delete"
                    },
                    "task_id": {
                        "type": "string",
                        "description": "Task ID (for update/complete/delete). Supports short prefixes."
                    },
                    "title": {
                        "type": "string",
                        "description": "Task title (for create/update)"
                    },
                    "description": {
                        "type": "string",
                        "description": "Task description"
                    },
                    "priority": {
                        "type": "string",
                        "description": "Priority: low/medium/high/urgent"
                    },
                    "status": {
                        "type": "string",
                        "description": "Status: pending/in_progress/completed/blocked (for update)"
                    },
                    "parent_id": {
                        "type": "string",
                        "description": "Parent task ID for subtasks"
                    },
                    "notes": {
                        "type": "string",
                        "description": "Completion notes (for complete)"
                    },
                    "include_completed": {
                        "type": "boolean",
                        "description": "Include completed tasks in list (default false)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max results for list (default 20)"
                    }
                },
                "required": ["action"]
            }),
        },
        Tool {
            tool_type: "function".into(),
            name: "goal".into(),
            description: Some("Manage high-level goals with milestones. Actions: create, list, update, add_milestone, complete_milestone.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "description": "Action: create/list/update/add_milestone/complete_milestone"
                    },
                    "goal_id": {
                        "type": "string",
                        "description": "Goal ID (for update/add_milestone)"
                    },
                    "milestone_id": {
                        "type": "string",
                        "description": "Milestone ID (for complete_milestone)"
                    },
                    "title": {
                        "type": "string",
                        "description": "Title (for create/update/add_milestone)"
                    },
                    "description": {
                        "type": "string",
                        "description": "Description"
                    },
                    "success_criteria": {
                        "type": "string",
                        "description": "Success criteria (for create)"
                    },
                    "priority": {
                        "type": "string",
                        "description": "Priority: low/medium/high/critical"
                    },
                    "status": {
                        "type": "string",
                        "description": "Status: planning/in_progress/blocked/completed/abandoned"
                    },
                    "progress_percent": {
                        "type": "integer",
                        "description": "Progress 0-100 (for update)"
                    },
                    "weight": {
                        "type": "integer",
                        "description": "Milestone weight for progress calculation"
                    },
                    "include_finished": {
                        "type": "boolean",
                        "description": "Include finished goals in list"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max results for list"
                    }
                },
                "required": ["action"]
            }),
        },
        Tool {
            tool_type: "function".into(),
            name: "correction".into(),
            description: Some("Record and manage corrections. When the user corrects your approach, record it to avoid the same mistake. Actions: record, list, validate.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "description": "Action: record/list/validate"
                    },
                    "correction_id": {
                        "type": "string",
                        "description": "Correction ID (for validate)"
                    },
                    "correction_type": {
                        "type": "string",
                        "description": "Type: style/approach/pattern/preference/anti_pattern"
                    },
                    "what_was_wrong": {
                        "type": "string",
                        "description": "What you did wrong (for record)"
                    },
                    "what_is_right": {
                        "type": "string",
                        "description": "What you should do instead (for record)"
                    },
                    "rationale": {
                        "type": "string",
                        "description": "Why this is the right approach"
                    },
                    "scope": {
                        "type": "string",
                        "description": "Scope: global/project/file/topic"
                    },
                    "keywords": {
                        "type": "string",
                        "description": "Comma-separated keywords for matching"
                    },
                    "outcome": {
                        "type": "string",
                        "description": "Outcome for validate: validated/deprecated"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max results for list"
                    }
                },
                "required": ["action"]
            }),
        },
        Tool {
            tool_type: "function".into(),
            name: "store_decision".into(),
            description: Some("Store an important architectural or design decision with context. Decisions are recalled semantically when relevant.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "key": {
                        "type": "string",
                        "description": "Unique key for this decision (e.g., 'auth-method', 'database-choice')"
                    },
                    "decision": {
                        "type": "string",
                        "description": "The decision that was made"
                    },
                    "category": {
                        "type": "string",
                        "description": "Category: architecture/design/tech-stack/workflow"
                    },
                    "context": {
                        "type": "string",
                        "description": "Context and rationale for the decision"
                    }
                },
                "required": ["key", "decision"]
            }),
        },
        Tool {
            tool_type: "function".into(),
            name: "record_rejected_approach".into(),
            description: Some("Record an approach that was tried and rejected. This prevents re-suggesting failed approaches in similar contexts.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "problem_context": {
                        "type": "string",
                        "description": "What problem you were trying to solve"
                    },
                    "approach": {
                        "type": "string",
                        "description": "The approach that was tried"
                    },
                    "rejection_reason": {
                        "type": "string",
                        "description": "Why this approach was rejected"
                    },
                    "related_files": {
                        "type": "string",
                        "description": "Comma-separated related file paths"
                    },
                    "related_topics": {
                        "type": "string",
                        "description": "Comma-separated related topics"
                    }
                },
                "required": ["problem_context", "approach", "rejection_reason"]
            }),
        },
        // ================================================================
        // Git Tools
        // ================================================================
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
            name: "git_commit".into(),
            description: Some("Create a git commit. Optionally stage all changes first.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "message": {
                        "type": "string",
                        "description": "Commit message"
                    },
                    "add_all": {
                        "type": "boolean",
                        "description": "Stage all changes before committing (git add -A)"
                    }
                },
                "required": ["message"]
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
        // ================================================================
        // Test Tools
        // ================================================================
        Tool {
            tool_type: "function".into(),
            name: "run_tests".into(),
            description: Some("Run tests for the project. Auto-detects runner (cargo/pytest/npm/go) or specify explicitly.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "runner": {
                        "type": "string",
                        "description": "Test runner: cargo/pytest/npm/go (auto-detected if omitted)"
                    },
                    "filter": {
                        "type": "string",
                        "description": "Filter tests by name/pattern"
                    },
                    "verbose": {
                        "type": "boolean",
                        "description": "Show verbose output (default false)"
                    }
                },
                "required": []
            }),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_tools() {
        let tools = get_tools();
        // 10 core tools + 5 power armor tools + 4 git tools + 1 test tool = 20
        assert_eq!(tools.len(), 20);
        assert_eq!(tools[0].name, "read_file");
        assert_eq!(tools[5].name, "edit_file");
        assert_eq!(tools[8].name, "remember");
        assert_eq!(tools[9].name, "recall");
        // Power armor tools
        assert_eq!(tools[10].name, "task");
        assert_eq!(tools[11].name, "goal");
        assert_eq!(tools[12].name, "correction");
        assert_eq!(tools[13].name, "store_decision");
        assert_eq!(tools[14].name, "record_rejected_approach");
        // Git tools
        assert_eq!(tools[15].name, "git_status");
        assert_eq!(tools[16].name, "git_diff");
        assert_eq!(tools[17].name, "git_commit");
        assert_eq!(tools[18].name, "git_log");
        // Test tools
        assert_eq!(tools[19].name, "run_tests");
    }
}
