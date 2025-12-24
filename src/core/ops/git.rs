//! Core git operations - shared by MCP and Chat
//!
//! Combines basic git commands (status, diff, commit, log) with
//! git intelligence (commits DB, cochange patterns, error fixes).

use std::path::PathBuf;
use std::process::Command;

use super::super::{CoreError, CoreResult, OpContext};

// ============================================================================
// Input/Output Types - Basic Git Operations
// ============================================================================

pub struct GitStatusInput {
    pub cwd: PathBuf,
}

pub struct GitStatusOutput {
    pub branch: String,
    pub staged: Vec<FileChange>,
    pub unstaged: Vec<FileChange>,
    pub untracked: Vec<String>,
    pub clean: bool,
}

pub struct FileChange {
    pub status: String,
    pub path: String,
}

pub struct GitDiffInput {
    pub cwd: PathBuf,
    pub staged: bool,
    pub path: Option<String>,
}

pub struct GitCommitInput {
    pub cwd: PathBuf,
    pub message: String,
    pub add_all: bool,
}

pub struct GitLogInput {
    pub cwd: PathBuf,
    pub limit: usize,
    pub path: Option<String>,
}

// ============================================================================
// Input/Output Types - Git Intelligence
// ============================================================================

pub struct GetRecentCommitsInput {
    pub file_path: Option<String>,
    pub author: Option<String>,
    pub limit: i64,
}

pub struct CommitInfo {
    pub commit_hash: String,
    pub author: Option<String>,
    pub email: Option<String>,
    pub message: String,
    pub files_changed: Option<String>,
    pub insertions: i64,
    pub deletions: i64,
    pub committed_at: String,
}

pub struct SearchCommitsInput {
    pub query: String,
    pub limit: i64,
}

pub struct FindCochangeInput {
    pub file_path: String,
    pub limit: i64,
}

pub struct CochangePattern {
    pub file: String,
    pub cochange_count: i64,
    pub confidence: f64,
    pub last_seen: String,
}

// ============================================================================
// Basic Git Operations
// ============================================================================

/// Get git status
pub fn git_status(input: GitStatusInput) -> CoreResult<GitStatusOutput> {
    let output = Command::new("git")
        .current_dir(&input.cwd)
        .args(["status", "--porcelain=v2", "--branch"])
        .output()
        .map_err(|e| CoreError::ShellExec("git status".to_string(), e.to_string()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(CoreError::ShellExec("git status".to_string(), stderr.to_string()));
    }

    let raw = String::from_utf8_lossy(&output.stdout);
    Ok(parse_status(&raw))
}

fn parse_status(raw: &str) -> GitStatusOutput {
    let mut branch = String::new();
    let mut staged = Vec::new();
    let mut unstaged = Vec::new();
    let mut untracked = Vec::new();

    for line in raw.lines() {
        if line.starts_with("# branch.head") {
            branch = line.split_whitespace().last().unwrap_or("").to_string();
        } else if line.starts_with("1 ") || line.starts_with("2 ") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 9 {
                let xy = parts[1];
                let path = parts[8];

                let x = xy.chars().next().unwrap_or('.');
                let y = xy.chars().nth(1).unwrap_or('.');

                if x != '.' {
                    staged.push(FileChange {
                        status: status_label(x).to_string(),
                        path: path.to_string(),
                    });
                }
                if y != '.' {
                    unstaged.push(FileChange {
                        status: status_label(y).to_string(),
                        path: path.to_string(),
                    });
                }
            }
        } else if line.starts_with("? ") {
            let path = line.trim_start_matches("? ");
            untracked.push(path.to_string());
        }
    }

    let clean = staged.is_empty() && unstaged.is_empty() && untracked.is_empty();

    GitStatusOutput {
        branch,
        staged,
        unstaged,
        untracked,
        clean,
    }
}

fn status_label(c: char) -> &'static str {
    match c {
        'M' => "modified",
        'A' => "added",
        'D' => "deleted",
        'R' => "renamed",
        'C' => "copied",
        'T' => "typechange",
        'U' => "unmerged",
        _ => "changed",
    }
}

/// Get git diff
pub fn git_diff(input: GitDiffInput) -> CoreResult<String> {
    let mut cmd = Command::new("git");
    cmd.current_dir(&input.cwd);
    cmd.args(["diff", "--stat", "--patch"]);

    if input.staged {
        cmd.arg("--staged");
    }

    if let Some(ref p) = input.path {
        cmd.arg("--").arg(p);
    }

    let output = cmd.output()
        .map_err(|e| CoreError::ShellExec("git diff".to_string(), e.to_string()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(CoreError::ShellExec("git diff".to_string(), stderr.to_string()));
    }

    let diff = String::from_utf8_lossy(&output.stdout);
    if diff.is_empty() {
        return Ok("No changes".to_string());
    }

    // Truncate if too long
    let lines: Vec<&str> = diff.lines().collect();
    if lines.len() > 100 {
        let truncated: String = lines[..100].join("\n");
        Ok(format!("{}\n... ({} more lines)", truncated, lines.len() - 100))
    } else {
        Ok(diff.into_owned())
    }
}

/// Create a git commit
pub fn git_commit(input: GitCommitInput) -> CoreResult<String> {
    // Optionally stage all changes
    if input.add_all {
        let add_output = Command::new("git")
            .current_dir(&input.cwd)
            .args(["add", "-A"])
            .output()
            .map_err(|e| CoreError::ShellExec("git add".to_string(), e.to_string()))?;

        if !add_output.status.success() {
            let stderr = String::from_utf8_lossy(&add_output.stderr);
            return Err(CoreError::ShellExec("git add".to_string(), stderr.to_string()));
        }
    }

    // Check if there's anything to commit
    let status = Command::new("git")
        .current_dir(&input.cwd)
        .args(["diff", "--staged", "--quiet"])
        .status()
        .map_err(|e| CoreError::ShellExec("git diff".to_string(), e.to_string()))?;

    if status.success() {
        return Ok("Nothing staged to commit".to_string());
    }

    // Create commit
    let output = Command::new("git")
        .current_dir(&input.cwd)
        .args(["commit", "-m", &input.message])
        .output()
        .map_err(|e| CoreError::ShellExec("git commit".to_string(), e.to_string()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(CoreError::ShellExec("git commit".to_string(), stderr.to_string()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.lines().take(5).collect::<Vec<_>>().join("\n"))
}

/// Get git log
pub fn git_log(input: GitLogInput) -> CoreResult<String> {
    let mut cmd = Command::new("git");
    cmd.current_dir(&input.cwd);
    cmd.args([
        "log",
        "--oneline",
        "--decorate",
        &format!("-{}", input.limit),
    ]);

    if let Some(ref p) = input.path {
        cmd.arg("--").arg(p);
    }

    let output = cmd.output()
        .map_err(|e| CoreError::ShellExec("git log".to_string(), e.to_string()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(CoreError::ShellExec("git log".to_string(), stderr.to_string()));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

// ============================================================================
// Git Intelligence Operations (Database-backed)
// ============================================================================

/// Get recent commits from database
pub async fn get_recent_commits(ctx: &OpContext, input: GetRecentCommitsInput) -> CoreResult<Vec<CommitInfo>> {
    let db = ctx.require_db()?;

    let query = r#"
        SELECT commit_hash, author_name, author_email, message, files_changed,
               insertions, deletions,
               datetime(committed_at, 'unixepoch', 'localtime') as committed_at
        FROM git_commits
        WHERE ($1 IS NULL OR files_changed LIKE $1)
          AND ($2 IS NULL OR author_email = $2)
        ORDER BY committed_at DESC
        LIMIT $3
    "#;

    let file_pattern = input.file_path.as_ref().map(|f| format!("%{}%", f));
    let rows = sqlx::query_as::<_, (String, Option<String>, Option<String>, String, Option<String>, i64, i64, String)>(query)
        .bind(&file_pattern)
        .bind(&input.author)
        .bind(input.limit)
        .fetch_all(db)
        .await?;

    Ok(rows.into_iter().map(|(hash, author, email, message, files, insertions, deletions, committed_at)| {
        CommitInfo {
            commit_hash: hash,
            author,
            email,
            message,
            files_changed: files,
            insertions,
            deletions,
            committed_at,
        }
    }).collect())
}

/// Search commits by message
pub async fn search_commits(ctx: &OpContext, input: SearchCommitsInput) -> CoreResult<Vec<CommitInfo>> {
    let db = ctx.require_db()?;
    let search_pattern = format!("%{}%", input.query);

    let query = r#"
        SELECT commit_hash, author_name, author_email, message, files_changed,
               0 as insertions, 0 as deletions,
               datetime(committed_at, 'unixepoch', 'localtime') as committed_at
        FROM git_commits
        WHERE message LIKE $1
        ORDER BY committed_at DESC
        LIMIT $2
    "#;

    let rows = sqlx::query_as::<_, (String, Option<String>, Option<String>, String, Option<String>, i64, i64, String)>(query)
        .bind(&search_pattern)
        .bind(input.limit)
        .fetch_all(db)
        .await?;

    Ok(rows.into_iter().map(|(hash, author, email, message, files, insertions, deletions, committed_at)| {
        CommitInfo {
            commit_hash: hash,
            author,
            email,
            message,
            files_changed: files,
            insertions,
            deletions,
            committed_at,
        }
    }).collect())
}

/// Find co-change patterns for a file
pub async fn find_cochange_patterns(ctx: &OpContext, input: FindCochangeInput) -> CoreResult<Vec<CochangePattern>> {
    let db = ctx.require_db()?;

    // Normalize path to relative
    let file_path = normalize_to_relative(&input.file_path);

    let query = r#"
        SELECT
            CASE WHEN file_a = $1 THEN file_b ELSE file_a END as related_file,
            cochange_count,
            confidence,
            datetime(last_seen, 'unixepoch', 'localtime') as last_seen
        FROM cochange_patterns
        WHERE file_a = $1 OR file_b = $1
        ORDER BY confidence DESC
        LIMIT $2
    "#;

    let rows = sqlx::query_as::<_, (String, i64, f64, String)>(query)
        .bind(&file_path)
        .bind(input.limit)
        .fetch_all(db)
        .await?;

    Ok(rows.into_iter().map(|(file, count, confidence, last_seen)| {
        CochangePattern {
            file,
            cochange_count: count,
            confidence,
            last_seen,
        }
    }).collect())
}

/// Normalize an absolute path to a relative path for database lookups
fn normalize_to_relative(path: &str) -> String {
    if path.starts_with('/') {
        // Try to find common directories
        for marker in ["src/", "lib/", "tests/", "examples/", "benches/"] {
            if let Some(idx) = path.find(marker) {
                return path[idx..].to_string();
            }
        }

        // Look for recognizable relative paths
        let parts: Vec<&str> = path.split('/').collect();
        for (i, part) in parts.iter().enumerate() {
            if *part == "src" || *part == "lib" || *part == "tests" {
                return parts[i..].join("/");
            }
        }

        // Last resort: use just the filename
        if let Some(last_slash) = path.rfind('/') {
            let filename = &path[last_slash + 1..];
            if !filename.is_empty() {
                return filename.to_string();
            }
        }
    }
    path.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_path() {
        assert_eq!(
            normalize_to_relative("/home/user/project/src/main.rs"),
            "src/main.rs"
        );
        assert_eq!(
            normalize_to_relative("/home/user/project/tests/test.rs"),
            "tests/test.rs"
        );
        assert_eq!(
            normalize_to_relative("src/lib.rs"),
            "src/lib.rs"
        );
    }

    #[test]
    fn test_parse_status() {
        let raw = r#"# branch.head main
# branch.upstream origin/main
1 M. N... 100644 100644 100644 abc123 abc124 src/main.rs
? untracked.txt"#;

        let status = parse_status(raw);
        assert_eq!(status.branch, "main");
        assert_eq!(status.staged.len(), 1);
        assert_eq!(status.staged[0].status, "modified");
        assert_eq!(status.untracked.len(), 1);
    }
}
