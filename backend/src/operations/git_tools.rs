// src/operations/git_tools.rs
// Git tool schemas for exposing git operations to DeepSeek

use serde_json::{json, Value};

/// Get all git operation tool schemas
pub fn get_git_tools() -> Vec<Value> {
    vec![
        git_history_tool(),
        git_blame_tool(),
        git_diff_tool(),
        git_file_history_tool(),
        git_branches_tool(),
        git_show_commit_tool(),
        git_file_at_commit_tool(),
        git_recent_changes_tool(),
        git_contributors_tool(),
        git_status_tool(),
    ]
}

/// Internal git history tool
fn git_history_tool() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "git_history_internal",
            "description": "Get commit history with author, date, message, and stats. Filter by branch, date range, author, or file path.",
            "parameters": {
                "type": "object",
                "properties": {
                    "branch": {
                        "type": "string",
                        "description": "Branch name (default: current branch)"
                    },
                    "limit": {
                        "type": "string",
                        "description": "Maximum number of commits to return (default: 20)"
                    },
                    "author": {
                        "type": "string",
                        "description": "Filter by author name or email"
                    },
                    "file_path": {
                        "type": "string",
                        "description": "Show only commits that affected this file"
                    },
                    "since": {
                        "type": "string",
                        "description": "Show commits since date (e.g., '2024-01-01', '1 week ago')"
                    }
                },
                "required": []
            }
        }
    })
}

/// Internal git blame tool
fn git_blame_tool() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "git_blame_internal",
            "description": "Show who last modified each line of a file, with commit hash, author, and date.",
            "parameters": {
                "type": "object",
                "properties": {
                    "file_path": {
                        "type": "string",
                        "description": "Path to the file to blame"
                    },
                    "start_line": {
                        "type": "string",
                        "description": "Start line number (optional)"
                    },
                    "end_line": {
                        "type": "string",
                        "description": "End line number (optional)"
                    }
                },
                "required": ["file_path"]
            }
        }
    })
}

/// Internal git diff tool
fn git_diff_tool() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "git_diff_internal",
            "description": "Show differences between commits, branches, or working tree. Returns added/removed/modified lines.",
            "parameters": {
                "type": "object",
                "properties": {
                    "from": {
                        "type": "string",
                        "description": "Commit hash or branch name to compare from"
                    },
                    "to": {
                        "type": "string",
                        "description": "Commit hash or branch name to compare to (default: working tree)"
                    },
                    "file_path": {
                        "type": "string",
                        "description": "Show diff for specific file only"
                    }
                },
                "required": []
            }
        }
    })
}

/// Internal git file history tool
fn git_file_history_tool() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "git_file_history_internal",
            "description": "Show all commits that modified a specific file, tracking renames and moves.",
            "parameters": {
                "type": "object",
                "properties": {
                    "file_path": {
                        "type": "string",
                        "description": "Path to the file"
                    },
                    "limit": {
                        "type": "string",
                        "description": "Maximum number of commits to return (default: 20)"
                    }
                },
                "required": ["file_path"]
            }
        }
    })
}

/// Internal git branches tool
fn git_branches_tool() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "git_branches_internal",
            "description": "List all branches with last commit info and ahead/behind counts relative to main branch.",
            "parameters": {
                "type": "object",
                "properties": {
                    "include_remote": {
                        "type": "string",
                        "description": "Include remote branches (default: false)"
                    }
                },
                "required": []
            }
        }
    })
}

/// Internal git show commit tool
fn git_show_commit_tool() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "git_show_commit_internal",
            "description": "Show detailed information about a specific commit including full diff and all files changed.",
            "parameters": {
                "type": "object",
                "properties": {
                    "commit_hash": {
                        "type": "string",
                        "description": "Commit hash (full or short)"
                    }
                },
                "required": ["commit_hash"]
            }
        }
    })
}

/// Internal git file at commit tool
fn git_file_at_commit_tool() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "git_file_at_commit_internal",
            "description": "Get the content of a file as it existed at a specific commit.",
            "parameters": {
                "type": "object",
                "properties": {
                    "file_path": {
                        "type": "string",
                        "description": "Path to the file"
                    },
                    "commit_hash": {
                        "type": "string",
                        "description": "Commit hash or branch name"
                    }
                },
                "required": ["file_path", "commit_hash"]
            }
        }
    })
}

/// Internal git recent changes tool
fn git_recent_changes_tool() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "git_recent_changes_internal",
            "description": "Show files modified in the last N commits or days. Highlights frequently changed files (hot spots).",
            "parameters": {
                "type": "object",
                "properties": {
                    "days": {
                        "type": "string",
                        "description": "Number of days to look back (default: 7)"
                    },
                    "limit": {
                        "type": "string",
                        "description": "Maximum number of commits to analyze (default: 50)"
                    }
                },
                "required": []
            }
        }
    })
}

/// Internal git contributors tool
fn git_contributors_tool() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "git_contributors_internal",
            "description": "Show who has contributed to the codebase, with commit counts and areas of expertise (which files they've worked on most).",
            "parameters": {
                "type": "object",
                "properties": {
                    "file_path": {
                        "type": "string",
                        "description": "Show contributors for specific file or directory"
                    },
                    "since": {
                        "type": "string",
                        "description": "Show contributions since date (e.g., '1 month ago')"
                    }
                },
                "required": []
            }
        }
    })
}

/// Internal git status tool
fn git_status_tool() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "git_status_internal",
            "description": "Show current working tree status: staged, unstaged, and untracked files. Also shows current branch and sync status with remote.",
            "parameters": {
                "type": "object",
                "properties": {},
                "required": []
            }
        }
    })
}
