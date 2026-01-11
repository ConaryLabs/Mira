// crates/mira-server/src/web/deepseek/tools.rs
// Tool definitions for DeepSeek chat

use super::types::Tool;

/// Get the Mira tools available to DeepSeek
pub fn mira_tools() -> Vec<Tool> {
    vec![
        Tool::function(
            "recall_memories",
            "Search semantic memory for relevant context, past decisions, and project knowledge",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Natural language query to search memories"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results (default 5)",
                        "default": 5
                    }
                },
                "required": ["query"]
            }),
        ),
        Tool::function(
            "search_code",
            "Semantic code search over the project codebase",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Natural language description of code to find"
                    },
                    "language": {
                        "type": "string",
                        "description": "Filter by programming language (e.g., 'rust', 'python')"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results (default 10)",
                        "default": 10
                    }
                },
                "required": ["query"]
            }),
        ),
        Tool::function(
            "find_callers",
            "Find all functions that call a specific function. Use this when user asks 'who calls X' or 'callers of X'.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "function_name": {
                        "type": "string",
                        "description": "Name of the function to find callers for"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results (default 20)",
                        "default": 20
                    }
                },
                "required": ["function_name"]
            }),
        ),
        Tool::function(
            "find_callees",
            "Find all functions called by a specific function. Use this when user asks 'what does X call' or 'callees of X'.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "function_name": {
                        "type": "string",
                        "description": "Name of the function to find callees for"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results (default 20)",
                        "default": 20
                    }
                },
                "required": ["function_name"]
            }),
        ),
        Tool::function(
            "list_tasks",
            "Get current tasks and their status for the project",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "status": {
                        "type": "string",
                        "enum": ["pending", "in_progress", "completed", "blocked"],
                        "description": "Filter by task status"
                    }
                },
                "required": []
            }),
        ),
        Tool::function(
            "list_goals",
            "Get project goals and their progress",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "status": {
                        "type": "string",
                        "enum": ["planning", "in_progress", "blocked", "completed", "abandoned"],
                        "description": "Filter by goal status"
                    }
                },
                "required": []
            }),
        ),
        Tool::function(
            "claude_task",
            "Send a coding task to Claude Code for the current project. Claude will edit files, run commands, and complete the task. Spawns a new instance if none exists.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "task": {
                        "type": "string",
                        "description": "The coding task for Claude Code to complete"
                    }
                },
                "required": ["task"]
            }),
        ),
        Tool::function(
            "claude_close",
            "Close the current project's Claude Code instance when done with coding tasks.",
            serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        ),
        Tool::function(
            "claude_status",
            "Check if Claude Code is running for the current project.",
            serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        ),
        Tool::function(
            "discuss",
            "Have a real-time conversation with Claude. Send a message and wait for Claude's structured response. Use this for code review, debugging together, getting Claude's expert analysis, or collaborating on complex tasks.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "message": {
                        "type": "string",
                        "description": "What to discuss with Claude"
                    }
                },
                "required": ["message"]
            }),
        ),
        Tool::function(
            "google_search",
            "Search the web using Google Custom Search. Returns titles, URLs, and snippets from search results.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The search query"
                    },
                    "num_results": {
                        "type": "integer",
                        "description": "Number of results to return (1-10, default 5)",
                        "default": 5
                    }
                },
                "required": ["query"]
            }),
        ),
        Tool::function(
            "web_fetch",
            "Fetch and extract content from a web page. Returns the page title and main text content.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "The URL to fetch"
                    }
                },
                "required": ["url"]
            }),
        ),
        Tool::function(
            "research",
            "Research a topic by searching the web, reading top results, and synthesizing findings into a grounded answer with citations. Use this when you need current information, technical comparisons, or factual verification.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "question": {
                        "type": "string",
                        "description": "The question or topic to research"
                    },
                    "depth": {
                        "type": "string",
                        "enum": ["quick", "thorough"],
                        "description": "Research depth: 'quick' (1 query, 3 pages) or 'thorough' (3 queries, 5 pages)",
                        "default": "quick"
                    }
                },
                "required": ["question"]
            }),
        ),
        Tool::function(
            "bash",
            "Execute shell commands on the system. Use for file operations, git, builds, system tasks, and anything outside of code editing.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The bash command to execute"
                    },
                    "working_directory": {
                        "type": "string",
                        "description": "Working directory for the command (defaults to project root)"
                    },
                    "timeout_seconds": {
                        "type": "integer",
                        "description": "Command timeout in seconds (default 60)",
                        "default": 60
                    }
                },
                "required": ["command"]
            }),
        ),
        Tool::function(
            "set_project",
            "Switch to a different project. Use when user wants to work on a specific project. The project context, tasks, goals, and memories will update accordingly.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "name_or_path": {
                        "type": "string",
                        "description": "Project name (e.g., 'Mira', 'website') or full path (e.g., '/home/user/projects/myapp')"
                    }
                },
                "required": ["name_or_path"]
            }),
        ),
        Tool::function(
            "list_projects",
            "List all known projects. Use to see what projects are available to switch to.",
            serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        ),
    ]
}
