//! Git-aware context tracking
//!
//! Polls git for recent activity to give chat context about "what just happened."
//! No background daemon - polls synchronously per request (~50ms).

use anyhow::Result;
use std::path::Path;
use std::process::Command;

/// Recent commit information
#[derive(Debug, Clone)]
pub struct CommitInfo {
    pub hash: String,
    pub message: String,
    pub relative_time: String,
}

/// File change information
#[derive(Debug, Clone)]
pub struct FileChange {
    pub path: String,
    pub insertions: i32,
    pub deletions: i32,
    pub is_new: bool,
}

/// Repository activity summary
#[derive(Debug, Clone, Default)]
pub struct RepoActivity {
    /// Recent commits (last 5)
    pub recent_commits: Vec<CommitInfo>,
    /// Files changed in recent commits
    pub changed_files: Vec<FileChange>,
    /// Whether there are uncommitted changes
    pub has_uncommitted: bool,
    /// Raw diff stat summary
    pub diff_summary: String,
}

impl RepoActivity {
    /// Check if there's any activity to report
    pub fn is_empty(&self) -> bool {
        self.recent_commits.is_empty() && self.changed_files.is_empty() && !self.has_uncommitted
    }

    /// Estimate token count for this activity
    pub fn estimate_tokens(&self) -> usize {
        // Rough estimate: 4 chars per token
        let chars = self.recent_commits.iter()
            .map(|c| c.hash.len() + c.message.len() + c.relative_time.len() + 20)
            .sum::<usize>()
            + self.changed_files.iter()
                .map(|f| f.path.len() + 30)
                .sum::<usize>()
            + self.diff_summary.len();
        chars / 4
    }
}

/// Get recent git activity for a repository
pub fn get_recent_activity(repo_path: &Path, commit_limit: usize) -> Result<RepoActivity> {
    let mut activity = RepoActivity::default();

    // Check if it's a git repo
    if !repo_path.join(".git").exists() {
        return Ok(activity);
    }

    // 1. Get recent commits
    activity.recent_commits = get_recent_commits(repo_path, commit_limit)?;

    // 2. Get changed files (diff stat for last N commits)
    let (files, summary) = get_changed_files(repo_path, commit_limit)?;
    activity.changed_files = files;
    activity.diff_summary = summary;

    // 3. Check for uncommitted changes
    activity.has_uncommitted = has_uncommitted_changes(repo_path)?;

    Ok(activity)
}

/// Get recent commits with hash, message, and relative time
fn get_recent_commits(repo_path: &Path, limit: usize) -> Result<Vec<CommitInfo>> {
    let output = Command::new("git")
        .args(["log", "-n", &limit.to_string(), "--pretty=format:%h|%s|%ar"])
        .current_dir(repo_path)
        .output()?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let commits = stdout
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.splitn(3, '|').collect();
            if parts.len() == 3 {
                Some(CommitInfo {
                    hash: parts[0].to_string(),
                    message: parts[1].to_string(),
                    relative_time: parts[2].to_string(),
                })
            } else {
                None
            }
        })
        .collect();

    Ok(commits)
}

/// Get files changed in recent commits with insertion/deletion counts
fn get_changed_files(repo_path: &Path, commit_limit: usize) -> Result<(Vec<FileChange>, String)> {
    // Get numstat for precise counts
    let numstat = Command::new("git")
        .args(["diff", &format!("HEAD~{}", commit_limit), "--numstat"])
        .current_dir(repo_path)
        .output();

    // Get stat for human-readable summary
    let stat = Command::new("git")
        .args(["diff", &format!("HEAD~{}", commit_limit), "--stat", "--stat-width=80"])
        .current_dir(repo_path)
        .output();

    let mut files = Vec::new();

    if let Ok(output) = numstat {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 3 {
                    let insertions = parts[0].parse().unwrap_or(0);
                    let deletions = parts[1].parse().unwrap_or(0);
                    let path = parts[2..].join(" ");
                    files.push(FileChange {
                        path,
                        insertions,
                        deletions,
                        is_new: deletions == 0 && insertions > 0,
                    });
                }
            }
        }
    }

    let summary = stat
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();

    Ok((files, summary))
}

/// Check if there are uncommitted changes (staged or unstaged)
fn has_uncommitted_changes(repo_path: &Path) -> Result<bool> {
    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(repo_path)
        .output()?;

    Ok(output.status.success() && !output.stdout.is_empty())
}

/// Get uncommitted changes summary (if any)
pub fn get_uncommitted_summary(repo_path: &Path) -> Result<Option<String>> {
    let output = Command::new("git")
        .args(["diff", "--stat", "--stat-width=80"])
        .current_dir(repo_path)
        .output()?;

    if output.status.success() && !output.stdout.is_empty() {
        Ok(Some(String::from_utf8_lossy(&output.stdout).to_string()))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_recent_activity() {
        // Test on current repo
        let activity = get_recent_activity(Path::new("."), 3).unwrap();
        // Should have some commits if we're in a git repo
        println!("Recent commits: {:?}", activity.recent_commits);
        println!("Changed files: {:?}", activity.changed_files);
        println!("Has uncommitted: {}", activity.has_uncommitted);
    }
}
