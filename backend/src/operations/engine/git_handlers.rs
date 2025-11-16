// src/operations/engine/git_handlers.rs
// Handlers for git operations - wraps git commands for LLM access

use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;
use tracing::{info, warn};

/// Handles git operations
pub struct GitHandlers {
    project_dir: PathBuf,
}

impl GitHandlers {
    pub fn new(project_dir: PathBuf) -> Self {
        Self { project_dir }
    }

    /// Execute a git tool call
    pub async fn execute_tool(&self, tool_name: &str, args: Value) -> Result<Value> {
        match tool_name {
            "git_history_internal" => self.git_history(args).await,
            "git_blame_internal" => self.git_blame(args).await,
            "git_diff_internal" => self.git_diff(args).await,
            "git_file_history_internal" => self.git_file_history(args).await,
            "git_branches_internal" => self.git_branches(args).await,
            "git_show_commit_internal" => self.git_show_commit(args).await,
            "git_file_at_commit_internal" => self.git_file_at_commit(args).await,
            "git_recent_changes_internal" => self.git_recent_changes(args).await,
            "git_contributors_internal" => self.git_contributors(args).await,
            "git_status_internal" => self.git_status(args).await,
            _ => Err(anyhow::anyhow!("Unknown git tool: {}", tool_name)),
        }
    }

    /// Execute a git command
    async fn run_git_command(&self, args: &[&str]) -> Result<String> {
        let output = Command::new("git")
            .args(args)
            .current_dir(&self.project_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .context("Failed to execute git command")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("Git command failed: {}", stderr));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Get commit history
    async fn git_history(&self, args: Value) -> Result<Value> {
        let branch = args.get("branch").and_then(|v| v.as_str());
        let limit = args
            .get("limit")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(20);
        let author = args.get("author").and_then(|v| v.as_str());
        let file_path = args.get("file_path").and_then(|v| v.as_str());
        let since = args.get("since").and_then(|v| v.as_str());

        info!("[GIT] Getting history (limit: {}, branch: {:?})", limit, branch);

        let limit_arg = format!("-{}", limit);
        let mut cmd_args = vec!["log", "--pretty=format:%H|%an|%ae|%ai|%s", &limit_arg];

        if let Some(b) = branch {
            cmd_args.push(b);
        }
        if let Some(a) = author {
            cmd_args.push("--author");
            cmd_args.push(a);
        }
        if let Some(s) = since {
            cmd_args.push("--since");
            cmd_args.push(s);
        }
        if let Some(f) = file_path {
            cmd_args.push("--");
            cmd_args.push(f);
        }

        match self.run_git_command(&cmd_args).await {
            Ok(output) => {
                let commits: Vec<Value> = output
                    .lines()
                    .filter(|line| !line.is_empty())
                    .map(|line| {
                        let parts: Vec<&str> = line.splitn(5, '|').collect();
                        json!({
                            "hash": parts.get(0).unwrap_or(&""),
                            "author": parts.get(1).unwrap_or(&""),
                            "email": parts.get(2).unwrap_or(&""),
                            "date": parts.get(3).unwrap_or(&""),
                            "message": parts.get(4).unwrap_or(&"")
                        })
                    })
                    .collect();

                Ok(json!({
                    "success": true,
                    "commits": commits,
                    "count": commits.len()
                }))
            }
            Err(e) => {
                warn!("[GIT] History failed: {}", e);
                Ok(json!({
                    "success": false,
                    "error": e.to_string()
                }))
            }
        }
    }

    /// Get git blame for a file
    async fn git_blame(&self, args: Value) -> Result<Value> {
        let file_path = args
            .get("file_path")
            .and_then(|v| v.as_str())
            .context("Missing file_path")?;
        let start_line = args.get("start_line").and_then(|v| v.as_str());
        let end_line = args.get("end_line").and_then(|v| v.as_str());

        info!("[GIT] Getting blame for {}", file_path);

        let mut cmd_args = vec!["blame", "--line-porcelain"];

        let line_range;
        if let (Some(start), Some(end)) = (start_line, end_line) {
            cmd_args.push("-L");
            line_range = format!("{},{}", start, end);
            cmd_args.push(&line_range);
        }

        cmd_args.push(file_path);

        match self.run_git_command(&cmd_args).await {
            Ok(output) => {
                // Parse porcelain format blame output
                let lines = output.lines().collect::<Vec<_>>();
                let mut blame_lines = Vec::new();
                let mut i = 0;

                while i < lines.len() {
                    if let Some(first_line) = lines.get(i) {
                        let parts: Vec<&str> = first_line.split_whitespace().collect();
                        if parts.len() >= 3 {
                            let commit_hash = parts[0];
                            let line_num = parts[2];

                            // Find author and author-time
                            let mut author = "";
                            let mut date = "";
                            let mut content = "";

                            for j in (i + 1)..lines.len() {
                                let line = lines[j];
                                if line.starts_with("author ") {
                                    author = line.strip_prefix("author ").unwrap_or("");
                                } else if line.starts_with("author-time ") {
                                    date = line.strip_prefix("author-time ").unwrap_or("");
                                } else if line.starts_with('\t') {
                                    content = line.strip_prefix('\t').unwrap_or("");
                                    i = j + 1;
                                    break;
                                }
                            }

                            blame_lines.push(json!({
                                "line": line_num,
                                "commit": &commit_hash[..7.min(commit_hash.len())],
                                "author": author,
                                "date": date,
                                "content": content
                            }));
                        }
                    }
                    i += 1;
                }

                Ok(json!({
                    "success": true,
                    "file": file_path,
                    "lines": blame_lines
                }))
            }
            Err(e) => {
                warn!("[GIT] Blame failed: {}", e);
                Ok(json!({
                    "success": false,
                    "error": e.to_string()
                }))
            }
        }
    }

    /// Get diff between commits/branches
    async fn git_diff(&self, args: Value) -> Result<Value> {
        let from = args.get("from").and_then(|v| v.as_str());
        let to = args.get("to").and_then(|v| v.as_str());
        let file_path = args.get("file_path").and_then(|v| v.as_str());

        info!("[GIT] Getting diff from {:?} to {:?}", from, to);

        let mut cmd_args = vec!["diff"];

        if let Some(f) = from {
            cmd_args.push(f);
            if let Some(t) = to {
                cmd_args.push(t);
            }
        }

        if let Some(path) = file_path {
            cmd_args.push("--");
            cmd_args.push(path);
        }

        match self.run_git_command(&cmd_args).await {
            Ok(output) => Ok(json!({
                "success": true,
                "diff": output,
                "from": from.unwrap_or("HEAD"),
                "to": to.unwrap_or("working tree")
            })),
            Err(e) => {
                warn!("[GIT] Diff failed: {}", e);
                Ok(json!({
                    "success": false,
                    "error": e.to_string()
                }))
            }
        }
    }

    /// Get file history
    async fn git_file_history(&self, args: Value) -> Result<Value> {
        let file_path = args
            .get("file_path")
            .and_then(|v| v.as_str())
            .context("Missing file_path")?;
        let limit = args
            .get("limit")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(20);

        info!("[GIT] Getting file history for {}", file_path);

        let cmd_args = [
            "log",
            "--follow",
            "--pretty=format:%H|%an|%ai|%s",
            &format!("-{}", limit),
            "--",
            file_path,
        ];

        match self.run_git_command(&cmd_args).await {
            Ok(output) => {
                let commits: Vec<Value> = output
                    .lines()
                    .filter(|line| !line.is_empty())
                    .map(|line| {
                        let parts: Vec<&str> = line.splitn(4, '|').collect();
                        json!({
                            "hash": parts.get(0).unwrap_or(&""),
                            "author": parts.get(1).unwrap_or(&""),
                            "date": parts.get(2).unwrap_or(&""),
                            "message": parts.get(3).unwrap_or(&"")
                        })
                    })
                    .collect();

                Ok(json!({
                    "success": true,
                    "file": file_path,
                    "commits": commits,
                    "count": commits.len()
                }))
            }
            Err(e) => {
                warn!("[GIT] File history failed: {}", e);
                Ok(json!({
                    "success": false,
                    "error": e.to_string()
                }))
            }
        }
    }

    /// Get list of branches
    async fn git_branches(&self, args: Value) -> Result<Value> {
        let include_remote = args
            .get("include_remote")
            .and_then(|v| v.as_str())
            .map(|s| s == "true")
            .unwrap_or(false);

        info!("[GIT] Getting branches (remote: {})", include_remote);

        let cmd_args = if include_remote {
            vec!["branch", "-a", "-v"]
        } else {
            vec!["branch", "-v"]
        };

        match self.run_git_command(&cmd_args).await {
            Ok(output) => {
                let branches: Vec<Value> = output
                    .lines()
                    .filter(|line| !line.is_empty())
                    .map(|line| {
                        let is_current = line.starts_with('*');
                        let line = line.trim_start_matches('*').trim();
                        let parts: Vec<&str> = line.split_whitespace().collect();

                        json!({
                            "name": parts.get(0).unwrap_or(&""),
                            "current": is_current,
                            "commit": parts.get(1).unwrap_or(&""),
                            "message": parts.get(2..).map(|p| p.join(" ")).unwrap_or_default()
                        })
                    })
                    .collect();

                Ok(json!({
                    "success": true,
                    "branches": branches,
                    "count": branches.len()
                }))
            }
            Err(e) => {
                warn!("[GIT] Branches failed: {}", e);
                Ok(json!({
                    "success": false,
                    "error": e.to_string()
                }))
            }
        }
    }

    /// Show commit details
    async fn git_show_commit(&self, args: Value) -> Result<Value> {
        let commit_hash = args
            .get("commit_hash")
            .and_then(|v| v.as_str())
            .context("Missing commit_hash")?;

        info!("[GIT] Showing commit {}", commit_hash);

        let cmd_args = ["show", commit_hash, "--stat"];

        match self.run_git_command(&cmd_args).await {
            Ok(output) => Ok(json!({
                "success": true,
                "commit": commit_hash,
                "details": output
            })),
            Err(e) => {
                warn!("[GIT] Show commit failed: {}", e);
                Ok(json!({
                    "success": false,
                    "error": e.to_string()
                }))
            }
        }
    }

    /// Get file content at specific commit
    async fn git_file_at_commit(&self, args: Value) -> Result<Value> {
        let file_path = args
            .get("file_path")
            .and_then(|v| v.as_str())
            .context("Missing file_path")?;
        let commit_hash = args
            .get("commit_hash")
            .and_then(|v| v.as_str())
            .context("Missing commit_hash")?;

        info!("[GIT] Getting {} at commit {}", file_path, commit_hash);

        let spec = format!("{}:{}", commit_hash, file_path);
        let cmd_args = ["show", &spec];

        match self.run_git_command(&cmd_args).await {
            Ok(content) => Ok(json!({
                "success": true,
                "file": file_path,
                "commit": commit_hash,
                "content": content
            })),
            Err(e) => {
                warn!("[GIT] File at commit failed: {}", e);
                Ok(json!({
                    "success": false,
                    "error": e.to_string()
                }))
            }
        }
    }

    /// Get recent changes
    async fn git_recent_changes(&self, args: Value) -> Result<Value> {
        let days = args
            .get("days")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(7);
        let limit = args
            .get("limit")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(50);

        info!("[GIT] Getting recent changes (last {} days)", days);

        let since = format!("{} days ago", days);
        let cmd_args = [
            "log",
            "--name-only",
            "--pretty=format:%H|%ai",
            &format!("-{}", limit),
            "--since",
            &since,
        ];

        match self.run_git_command(&cmd_args).await {
            Ok(output) => {
                let mut file_changes: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
                let lines = output.lines().collect::<Vec<_>>();

                for line in lines {
                    if !line.contains('|') && !line.is_empty() {
                        // This is a filename
                        *file_changes.entry(line.to_string()).or_insert(0) += 1;
                    }
                }

                let mut hotspots: Vec<Value> = file_changes
                    .iter()
                    .map(|(file, count)| json!({
                        "file": file,
                        "changes": count
                    }))
                    .collect();

                hotspots.sort_by(|a, b| {
                    b["changes"].as_u64().unwrap_or(0).cmp(&a["changes"].as_u64().unwrap_or(0))
                });

                Ok(json!({
                    "success": true,
                    "since": since,
                    "hotspots": hotspots,
                    "total_files": hotspots.len()
                }))
            }
            Err(e) => {
                warn!("[GIT] Recent changes failed: {}", e);
                Ok(json!({
                    "success": false,
                    "error": e.to_string()
                }))
            }
        }
    }

    /// Get contributors
    async fn git_contributors(&self, args: Value) -> Result<Value> {
        let file_path = args.get("file_path").and_then(|v| v.as_str());
        let since = args.get("since").and_then(|v| v.as_str());

        info!("[GIT] Getting contributors");

        let mut cmd_args = vec!["shortlog", "-sn"];

        if let Some(s) = since {
            cmd_args.push("--since");
            cmd_args.push(s);
        }

        if let Some(path) = file_path {
            cmd_args.push("--");
            cmd_args.push(path);
        }

        match self.run_git_command(&cmd_args).await {
            Ok(output) => {
                let contributors: Vec<Value> = output
                    .lines()
                    .filter(|line| !line.is_empty())
                    .map(|line| {
                        let parts: Vec<&str> = line.trim().splitn(2, '\t').collect();
                        json!({
                            "commits": parts.get(0).unwrap_or(&"0").parse::<u32>().unwrap_or(0),
                            "author": parts.get(1).unwrap_or(&"")
                        })
                    })
                    .collect();

                Ok(json!({
                    "success": true,
                    "contributors": contributors,
                    "count": contributors.len(),
                    "file": file_path
                }))
            }
            Err(e) => {
                warn!("[GIT] Contributors failed: {}", e);
                Ok(json!({
                    "success": false,
                    "error": e.to_string()
                }))
            }
        }
    }

    /// Get git status
    async fn git_status(&self, _args: Value) -> Result<Value> {
        info!("[GIT] Getting status");

        match self.run_git_command(&["status", "--porcelain", "-b"]).await {
            Ok(output) => {
                let lines: Vec<&str> = output.lines().collect();
                let branch = lines
                    .get(0)
                    .and_then(|l| l.strip_prefix("## "))
                    .unwrap_or("unknown");

                let mut staged = Vec::new();
                let mut unstaged = Vec::new();
                let mut untracked = Vec::new();

                for line in lines.iter().skip(1) {
                    if line.len() < 3 {
                        continue;
                    }

                    let status = &line[0..2];
                    let file = line[3..].trim();

                    match status {
                        "??" => untracked.push(file),
                        s if s.chars().next().unwrap() != ' ' => staged.push(file),
                        s if s.chars().nth(1).unwrap() != ' ' => unstaged.push(file),
                        _ => {}
                    }
                }

                Ok(json!({
                    "success": true,
                    "branch": branch,
                    "staged": staged,
                    "unstaged": unstaged,
                    "untracked": untracked,
                    "clean": staged.is_empty() && unstaged.is_empty() && untracked.is_empty()
                }))
            }
            Err(e) => {
                warn!("[GIT] Status failed: {}", e);
                Ok(json!({
                    "success": false,
                    "error": e.to_string()
                }))
            }
        }
    }
}
