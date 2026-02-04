// crates/mira-server/src/git/commit.rs
// Git commit operations using CLI

use super::{git_cmd, git_cmd_opt};
use std::path::Path;

/// A commit from git log with metadata
#[derive(Debug)]
pub struct GitCommit {
    pub hash: String,
    pub timestamp: i64,
    pub message: String,
}

/// Get the current git HEAD commit hash (full 40-char SHA).
pub fn get_git_head(project_path: &str) -> Option<String> {
    let hash = git_cmd_opt(Path::new(project_path), &["rev-parse", "HEAD"])?;
    if hash.len() == 40 && hash.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(hash)
    } else {
        None
    }
}

/// Check if a commit is an ancestor of HEAD (handles rebases, force pushes).
pub fn is_ancestor(project_path: &str, commit: &str) -> bool {
    git_cmd(
        Path::new(project_path),
        &["merge-base", "--is-ancestor", commit, "HEAD"],
    )
    .is_ok()
}

/// Get recent commits with hash, unix timestamp, and subject
pub fn get_recent_commits(project_path: &str, limit: usize) -> Vec<GitCommit> {
    match git_cmd_opt(
        Path::new(project_path),
        &["log", &format!("-{}", limit), "--format=%H %ct %s"],
    ) {
        Some(output) => parse_commit_lines(&output),
        None => vec![],
    }
}

/// Get commits after a given commit up to a timestamp
pub fn get_commits_in_range(
    project_path: &str,
    after_commit: &str,
    before_timestamp: i64,
) -> Vec<GitCommit> {
    match git_cmd_opt(
        Path::new(project_path),
        &[
            "log",
            "--format=%H %ct %s",
            &format!("--before={}", before_timestamp),
            &format!("{}..HEAD", after_commit),
        ],
    ) {
        Some(output) => parse_commit_lines(&output),
        None => vec![],
    }
}

/// Parse "HASH TIMESTAMP SUBJECT" lines into GitCommit structs
pub fn parse_commit_lines(output: &str) -> Vec<GitCommit> {
    output
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            // Format: "HASH TIMESTAMP SUBJECT..."
            let mut parts = line.splitn(3, ' ');
            let hash = parts.next()?.to_string();
            let timestamp: i64 = parts.next()?.parse().ok()?;
            let message = parts.next().unwrap_or("").to_string();

            // Validate hash is 40 hex chars
            if hash.len() != 40 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
                return None;
            }

            Some(GitCommit {
                hash,
                timestamp,
                message,
            })
        })
        .collect()
}

/// Get the unix timestamp for a specific commit
pub fn get_commit_timestamp(project_path: &str, commit_hash: &str) -> Option<i64> {
    git_cmd_opt(
        Path::new(project_path),
        &["log", "-1", "--format=%ct", commit_hash],
    )?
    .parse()
    .ok()
}

/// Get the commit message (subject line) for a specific commit
pub fn get_commit_message(project_path: &str, commit_hash: &str) -> Option<String> {
    let msg = git_cmd_opt(
        Path::new(project_path),
        &["log", "-1", "--format=%s", commit_hash],
    )?;
    if msg.is_empty() { None } else { Some(msg) }
}

/// Get the list of files changed in a specific commit
pub fn get_files_for_commit(project_path: &str, commit_hash: &str) -> Vec<String> {
    match git_cmd_opt(
        Path::new(project_path),
        &["diff-tree", "--no-commit-id", "-r", "--name-only", commit_hash],
    ) {
        Some(output) => output
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.trim().to_string())
            .collect(),
        None => vec![],
    }
}

/// A commit with its associated file list (for batch operations)
#[derive(Debug)]
pub struct CommitWithFiles {
    pub hash: String,
    pub timestamp: i64,
    pub message: String,
    pub files: Vec<String>,
}

/// Get recent commits with their changed files in a single git call.
///
/// Uses `git log --format=... --name-only` to avoid N+1 per-commit subprocess calls.
pub fn get_commits_with_files(project_path: &str, limit: usize) -> Vec<CommitWithFiles> {
    // Use a separator that won't appear in commit messages
    let output = git_cmd_opt(
        Path::new(project_path),
        &[
            "log",
            &format!("-{}", limit),
            "--format=\x1e%H %ct %s",
            "--name-only",
        ],
    );

    let Some(output) = output else {
        return vec![];
    };

    let mut commits = Vec::new();
    // Split on record separator (0x1e) which precedes each commit header
    for chunk in output.split('\x1e') {
        let chunk = chunk.trim();
        if chunk.is_empty() {
            continue;
        }

        let mut lines = chunk.lines();
        let header = match lines.next() {
            Some(h) => h.trim(),
            None => continue,
        };

        // Parse "HASH TIMESTAMP SUBJECT"
        let mut parts = header.splitn(3, ' ');
        let hash = match parts.next() {
            Some(h) if h.len() == 40 && h.chars().all(|c| c.is_ascii_hexdigit()) => h.to_string(),
            _ => continue,
        };
        let timestamp: i64 = match parts.next().and_then(|s| s.parse().ok()) {
            Some(ts) => ts,
            None => continue,
        };
        let message = parts.next().unwrap_or("").to_string();

        // Remaining lines are file names
        let files: Vec<String> = lines
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.trim().to_string())
            .collect();

        commits.push(CommitWithFiles {
            hash,
            timestamp,
            message,
            files,
        });
    }

    commits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_commit_lines_valid() {
        let hash1 = "a".repeat(40);
        let hash2 = "b".repeat(40);
        let input = format!(
            "{} 1706000000 feat: add feature\n{} 1706003600 fix: broken thing\n",
            hash1, hash2
        );
        let commits = parse_commit_lines(&input);
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].hash, hash1);
        assert_eq!(commits[0].timestamp, 1706000000);
        assert_eq!(commits[0].message, "feat: add feature");
        assert_eq!(commits[1].hash, hash2);
        assert_eq!(commits[1].message, "fix: broken thing");
    }

    #[test]
    fn test_parse_commit_lines_empty() {
        assert!(parse_commit_lines("").is_empty());
        assert!(parse_commit_lines("   \n  \n").is_empty());
    }

    #[test]
    fn test_parse_commit_lines_invalid_hash() {
        // Short hash should be rejected
        let input = "abc123 1706000000 some commit\n";
        assert!(parse_commit_lines(input).is_empty());
    }

    #[test]
    fn test_parse_commit_lines_invalid_timestamp() {
        let hash = "a".repeat(40);
        let input = format!("{} notanumber some commit\n", hash);
        assert!(parse_commit_lines(&input).is_empty());
    }
}
