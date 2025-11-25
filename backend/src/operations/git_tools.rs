// src/operations/git_tools.rs
// Git tool schemas for GPT 5.1 using ToolBuilder

use serde_json::Value;

use crate::operations::tool_builder::{properties, ToolBuilder};

/// Get all git operation tool schemas
pub fn get_git_tools() -> Vec<Value> {
    vec![
        git_history(),
        git_blame(),
        git_diff(),
        git_file_history(),
        git_branches(),
        git_show_commit(),
        git_file_at_commit(),
        git_recent_changes(),
        git_contributors(),
        git_status(),
    ]
}

/// Get commit history with optional filters
fn git_history() -> Value {
    ToolBuilder::new(
        "git_history",
        "Get commit history with author, date, message, and stats. Filter by branch, date range, author, or file path.",
    )
    .property("branch", properties::string("Branch name (default: current branch)"), false)
    .property("limit", properties::integer("Maximum commits to return", Some(20)), false)
    .property("author", properties::string("Filter by author name or email"), false)
    .property("file_path", properties::path("Show only commits affecting this file"), false)
    .property("since", properties::date("Show commits since date"), false)
    .build()
}

/// Show who last modified each line of a file
fn git_blame() -> Value {
    ToolBuilder::new(
        "git_blame",
        "Show who last modified each line of a file with commit hash, author, and date.",
    )
    .property("file_path", properties::path("Path to the file to blame"), true)
    .property("start_line", properties::integer("Start line number", None), false)
    .property("end_line", properties::integer("End line number", None), false)
    .build()
}

/// Show differences between commits, branches, or working tree
fn git_diff() -> Value {
    ToolBuilder::new(
        "git_diff",
        "Show differences between commits, branches, or working tree. Returns added/removed/modified lines.",
    )
    .property("from", properties::commit_hash("Commit or branch to compare from"), false)
    .property("to", properties::commit_hash("Commit or branch to compare to (default: working tree)"), false)
    .property("file_path", properties::path("Show diff for specific file only"), false)
    .build()
}

/// Show commits that modified a specific file
fn git_file_history() -> Value {
    ToolBuilder::new(
        "git_file_history",
        "Show all commits that modified a specific file, tracking renames and moves.",
    )
    .property("file_path", properties::path("Path to the file"), true)
    .property("limit", properties::integer("Maximum commits to return", Some(20)), false)
    .build()
}

/// List all branches with status
fn git_branches() -> Value {
    ToolBuilder::new(
        "git_branches",
        "List all branches with last commit info and ahead/behind counts relative to main branch.",
    )
    .property("include_remote", properties::boolean("Include remote branches", false), false)
    .build()
}

/// Show detailed commit information
fn git_show_commit() -> Value {
    ToolBuilder::new(
        "git_show_commit",
        "Show detailed information about a specific commit including full diff and all files changed.",
    )
    .property("commit_hash", properties::commit_hash("Commit hash to inspect"), true)
    .build()
}

/// Get file content at a specific commit
fn git_file_at_commit() -> Value {
    ToolBuilder::new(
        "git_file_at_commit",
        "Get the content of a file as it existed at a specific commit.",
    )
    .property("file_path", properties::path("Path to the file"), true)
    .property("commit_hash", properties::commit_hash("Commit hash or branch name"), true)
    .build()
}

/// Show recently changed files
fn git_recent_changes() -> Value {
    ToolBuilder::new(
        "git_recent_changes",
        "Show files modified in the last N days. Highlights frequently changed files (hot spots).",
    )
    .property("days", properties::integer("Number of days to look back", Some(7)), false)
    .property("limit", properties::integer("Maximum commits to analyze", Some(50)), false)
    .build()
}

/// Show contributors and their areas of expertise
fn git_contributors() -> Value {
    ToolBuilder::new(
        "git_contributors",
        "Show who has contributed to the codebase with commit counts and areas of expertise.",
    )
    .property("file_path", properties::path("Show contributors for specific file or directory"), false)
    .property("since", properties::date("Show contributions since date"), false)
    .build()
}

/// Show current working tree status
fn git_status() -> Value {
    ToolBuilder::new(
        "git_status",
        "Show current working tree status: staged, unstaged, and untracked files. Also shows current branch and sync status with remote.",
    )
    .build()
}
