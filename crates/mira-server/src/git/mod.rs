// crates/mira-server/src/git/mod.rs
// Centralized git operations
//
// - branch: git2-based branch detection and caching
// - commit: commit history, timestamps, file lists
// - diff: unified diffs, numstat parsing, staged/working diffs

mod branch;
mod commit;
mod diff;

pub use branch::{clear_branch_cache, get_git_branch, get_git_branch_uncached, is_git_repo};
pub use commit::{
    CommitWithFiles, GitCommit, get_commit_message, get_commit_timestamp, get_commits_in_range,
    get_commits_with_files, get_files_for_commit, get_git_head, get_recent_commits, is_ancestor,
    parse_commit_lines,
};
pub use diff::{
    derive_stats_from_unified_diff, get_head_commit, get_staged_diff, get_unified_diff,
    get_working_diff, parse_diff_stats, parse_numstat_output, parse_staged_stats,
    parse_working_stats, resolve_ref,
};

use std::path::Path;
use std::process::Command;

/// Validate that a git ref doesn't look like a CLI flag (defense-in-depth)
pub(crate) fn validate_ref(r: &str) -> Result<(), String> {
    if r.starts_with('-') {
        return Err(format!("Invalid git ref: '{}'", r));
    }
    if r.contains('\0') || r.contains('\n') || r.contains('\r') {
        return Err("Invalid git ref: contains forbidden characters".to_string());
    }
    Ok(())
}

/// Run a git command and return trimmed stdout, or an error.
pub(crate) fn git_cmd(project_path: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(project_path)
        .output()
        .map_err(|e| format!("Failed to run git {}: {}", args.first().unwrap_or(&""), e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "git {} failed: {}",
            args.first().unwrap_or(&""),
            stderr
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Run a git command, returning Some(stdout) on success or None on failure.
pub(crate) fn git_cmd_opt(project_path: &Path, args: &[&str]) -> Option<String> {
    git_cmd(project_path, args).ok()
}
