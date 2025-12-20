//! Git integration tools
//!
//! Thin wrapper delegating to core::ops::git for shared implementation.

use anyhow::Result;
use serde_json::Value;
use std::path::Path;

use crate::core::ops::git as core_git;

/// Git tool implementations
pub struct GitTools<'a> {
    pub cwd: &'a Path,
}

impl<'a> GitTools<'a> {
    /// Get git status (working tree state)
    pub async fn git_status(&self, _args: &Value) -> Result<String> {
        let input = core_git::GitStatusInput {
            cwd: self.cwd.to_path_buf(),
        };

        match core_git::git_status(input) {
            Ok(output) => Ok(format_status(&output)),
            Err(e) => Ok(format!("Error: {}", e)),
        }
    }

    /// Get git diff (staged or unstaged changes)
    pub async fn git_diff(&self, args: &Value) -> Result<String> {
        let staged = args["staged"].as_bool().unwrap_or(false);
        let path = args["path"].as_str().map(String::from);

        let input = core_git::GitDiffInput {
            cwd: self.cwd.to_path_buf(),
            staged,
            path,
        };

        match core_git::git_diff(input) {
            Ok(diff) => Ok(diff),
            Err(e) => Ok(format!("Error: {}", e)),
        }
    }

    /// Create a git commit
    pub async fn git_commit(&self, args: &Value) -> Result<String> {
        let message = args["message"].as_str().unwrap_or("Update");
        let add_all = args["add_all"].as_bool().unwrap_or(false);

        let input = core_git::GitCommitInput {
            cwd: self.cwd.to_path_buf(),
            message: message.to_string(),
            add_all,
        };

        match core_git::git_commit(input) {
            Ok(output) => Ok(output),
            Err(e) => Ok(format!("Error: {}", e)),
        }
    }

    /// Get recent commit log
    pub async fn git_log(&self, args: &Value) -> Result<String> {
        let limit = args["limit"].as_u64().unwrap_or(10) as usize;
        let path = args["path"].as_str().map(String::from);

        let input = core_git::GitLogInput {
            cwd: self.cwd.to_path_buf(),
            limit,
            path,
        };

        match core_git::git_log(input) {
            Ok(log) => Ok(log),
            Err(e) => Ok(format!("Error: {}", e)),
        }
    }
}

/// Format git status output for display
fn format_status(output: &core_git::GitStatusOutput) -> String {
    let mut result = format!("Branch: {}\n", output.branch);

    if !output.staged.is_empty() {
        result.push_str("\nStaged:\n");
        for f in &output.staged {
            result.push_str(&format!("  {}: {}\n", f.status, f.path));
        }
    }

    if !output.unstaged.is_empty() {
        result.push_str("\nUnstaged:\n");
        for f in &output.unstaged {
            result.push_str(&format!("  {}: {}\n", f.status, f.path));
        }
    }

    if !output.untracked.is_empty() {
        result.push_str("\nUntracked:\n");
        for f in &output.untracked {
            result.push_str(&format!("  {}\n", f));
        }
    }

    if output.clean {
        result.push_str("\nWorking tree clean");
    }

    result
}
