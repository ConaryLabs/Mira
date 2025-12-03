// src/operations/tools/git_analysis.rs
// Git analysis tool schemas for code history and collaboration

use serde_json::Value;
use crate::operations::tool_builder::{ToolBuilder, properties};
use super::common::with_limit;

/// Get all git analysis tool schemas
pub fn get_tools() -> Vec<Value> {
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

/// Get commit history with optional filters
pub fn git_history_tool() -> Value {
    let builder = ToolBuilder::new(
        "git_history",
        "Get commit history with author, date, and message. Filter by branch, author, file, or date range. Useful for understanding code evolution and finding when changes were made."
    )
    .property("branch", properties::string("Branch name (default: current branch)"), false)
    .property("author", properties::string("Filter by author name or email"), false)
    .property("file_path", properties::path("Show only commits affecting this file"), false)
    .property("since", properties::date("Show commits since date"), false);

    with_limit(builder, 20).build()
}

/// Show who last modified each line of a file
pub fn git_blame_tool() -> Value {
    ToolBuilder::new(
        "git_blame",
        "Show who last modified each line of a file with commit hash, author, and date. Perfect for understanding why code was changed and who to ask about it."
    )
    .property("file_path", properties::path("Path to the file to blame"), true)
    .property("start_line", properties::integer("Start line number", None), false)
    .property("end_line", properties::integer("End line number", None), false)
    .build()
}

/// Show differences between commits, branches, or working tree
pub fn git_diff_tool() -> Value {
    ToolBuilder::new(
        "git_diff",
        "Show differences between commits, branches, or working tree. Returns added/removed/modified lines. Useful for code review and understanding changes."
    )
    .property("from", properties::commit_hash("Commit hash or branch name to compare from"), false)
    .property("to", properties::commit_hash("Commit hash or branch to compare to (default: working tree)"), false)
    .property("file_path", properties::path("Show diff for specific file only"), false)
    .build()
}

/// Show commits that modified a specific file
pub fn git_file_history_tool() -> Value {
    let builder = ToolBuilder::new(
        "git_file_history",
        "Show all commits that modified a specific file, tracking renames and moves. Useful for understanding file evolution and finding when bugs were introduced."
    )
    .property("file_path", properties::path("Path to the file"), true);

    with_limit(builder, 20).build()
}

/// List all branches with status
pub fn git_branches_tool() -> Value {
    ToolBuilder::new(
        "git_branches",
        "List all branches with last commit info and ahead/behind counts. Useful for understanding branch status and finding stale branches."
    )
    .property("include_remote", properties::boolean("Include remote branches", false), false)
    .build()
}

/// Show detailed commit information
pub fn git_show_commit_tool() -> Value {
    ToolBuilder::new(
        "git_show_commit",
        "Show detailed information about a specific commit including full diff and all files changed. Useful for understanding what a commit did."
    )
    .property("commit_hash", properties::commit_hash("Commit hash to inspect"), true)
    .build()
}

/// Get file content at a specific commit
pub fn git_file_at_commit_tool() -> Value {
    ToolBuilder::new(
        "git_file_at_commit",
        "Get the content of a file as it existed at a specific commit. Compare with current version to see how it changed. Useful for debugging when code broke."
    )
    .property("file_path", properties::path("Path to the file"), true)
    .property("commit_hash", properties::commit_hash("Commit hash or branch name"), true)
    .build()
}

/// Show recently changed files
pub fn git_recent_changes_tool() -> Value {
    ToolBuilder::new(
        "git_recent_changes",
        "Show files modified in the last N commits or days. Highlights frequently changed files (hot spots) that may need attention. Useful for finding volatile code."
    )
    .property("days", properties::integer("Number of days to look back", Some(7)), false)
    .property("limit", properties::integer("Maximum commits to analyze", Some(50)), false)
    .build()
}

/// Show contributors and their areas of expertise
pub fn git_contributors_tool() -> Value {
    ToolBuilder::new(
        "git_contributors",
        "Show who has contributed to the codebase with commit counts. Optionally filter by file/directory or date range. Useful for finding domain experts."
    )
    .property("file_path", properties::path("Show contributors for specific file or directory"), false)
    .property("since", properties::date("Show contributions since date"), false)
    .build()
}

/// Show current working tree status
pub fn git_status_tool() -> Value {
    ToolBuilder::new(
        "git_status",
        "Show current working tree status: staged, unstaged, and untracked files. Also shows current branch and sync status with remote. Essential for understanding current repository state."
    )
    .build()
}
