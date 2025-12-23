//! Tool definitions for DeepSeek function calling

use serde::Serialize;
use serde_json::json;

/// Tool definition for function calling
#[derive(Debug, Clone, Serialize)]
pub struct Tool {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub parameters: serde_json::Value,
}

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
            description: Some("Fetch content from a URL and convert to text. Automatically falls back to Google Cache on 403 errors.".into()),
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
                    },
                    "use_cache": {
                        "type": "boolean",
                        "description": "Force fetch from Google Cache instead of direct URL (useful for sites that block bots)"
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
        // ================================================================
        // Artifact Tools
        // ================================================================
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
        // ================================================================
        // Council Tools - Consult other AI models
        // ================================================================
        Tool {
            tool_type: "function".into(),
            name: "council".into(),
            description: Some("Consult the council - calls GPT 5.2, Opus 4.5, and Gemini 3 Pro in parallel. Use for important decisions, architecture review, or when you want diverse perspectives.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "message": {
                        "type": "string",
                        "description": "The question or topic to consult the council about"
                    },
                    "context": {
                        "type": "string",
                        "description": "Optional context to include (code, previous discussion, etc.)"
                    }
                },
                "required": ["message"]
            }),
        },
        Tool {
            tool_type: "function".into(),
            name: "ask_gpt".into(),
            description: Some("Ask GPT 5.2 directly. Good for reasoning, analysis, and complex problem-solving.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "message": {
                        "type": "string",
                        "description": "The question or request for GPT 5.2"
                    },
                    "context": {
                        "type": "string",
                        "description": "Optional context to include"
                    }
                },
                "required": ["message"]
            }),
        },
        Tool {
            tool_type: "function".into(),
            name: "ask_opus".into(),
            description: Some("Ask Claude Opus 4.5 directly. Good for nuanced analysis, creative tasks, and detailed explanations.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "message": {
                        "type": "string",
                        "description": "The question or request for Opus 4.5"
                    },
                    "context": {
                        "type": "string",
                        "description": "Optional context to include"
                    }
                },
                "required": ["message"]
            }),
        },
        Tool {
            tool_type: "function".into(),
            name: "ask_gemini".into(),
            description: Some("Ask Gemini 3 Pro directly. Good for research, factual questions, and multi-modal tasks.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "message": {
                        "type": "string",
                        "description": "The question or request for Gemini 3 Pro"
                    },
                    "context": {
                        "type": "string",
                        "description": "Optional context to include"
                    }
                },
                "required": ["message"]
            }),
        },
        // ================================================================
        // Code Intelligence Tools
        // ================================================================
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
        // ================================================================
        // Git Intelligence Tools
        // ================================================================
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
        // ================================================================
        // Build Tracking Tools
        // ================================================================
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
        // ================================================================
        // Document Tools
        // ================================================================
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
        // ================================================================
        // Index Tools
        // ================================================================
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
        // ================================================================
        // Proactive Context Tool
        // ================================================================
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_tools() {
        let tools = get_tools();
        // 10 core + 5 power armor + 4 git + 1 test + 2 artifact + 4 council
        // + 5 code intel + 5 git intel + 1 build + 1 document + 1 index + 1 proactive = 40
        assert_eq!(tools.len(), 40);
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
        // Test tools
        assert_eq!(tools[19].name, "run_tests");
        // Council tools
        assert_eq!(tools[22].name, "council");
        // Code intel tools
        assert_eq!(tools[26].name, "get_symbols");
        assert_eq!(tools[27].name, "get_call_graph");
        // Git intel tools
        assert_eq!(tools[31].name, "get_recent_commits");
        // Build tracking
        assert_eq!(tools[36].name, "build");
        // Document management
        assert_eq!(tools[37].name, "document");
        // Index
        assert_eq!(tools[38].name, "index");
        // Proactive context
        assert_eq!(tools[39].name, "get_proactive_context");
    }
}
