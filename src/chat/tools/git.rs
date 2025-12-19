//! Git integration tools
//!
//! Provides git operations:
//! - git_status: Check working tree status
//! - git_diff: Show staged/unstaged changes
//! - git_commit: Create a commit

use anyhow::Result;
use serde_json::Value;
use std::path::Path;
use std::process::Command;

/// Git tool implementations
pub struct GitTools<'a> {
    pub cwd: &'a Path,
}

impl<'a> GitTools<'a> {
    /// Get git status (working tree state)
    pub async fn git_status(&self, _args: &Value) -> Result<String> {
        let output = Command::new("git")
            .current_dir(self.cwd)
            .args(["status", "--porcelain=v2", "--branch"])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Ok(format!("Error: {}", stderr));
        }

        let raw = String::from_utf8_lossy(&output.stdout);
        Ok(self.format_status(&raw))
    }

    /// Format porcelain v2 status into readable output
    fn format_status(&self, raw: &str) -> String {
        let mut branch = String::new();
        let mut staged = Vec::new();
        let mut unstaged = Vec::new();
        let mut untracked = Vec::new();

        for line in raw.lines() {
            if line.starts_with("# branch.head") {
                branch = line.split_whitespace().last().unwrap_or("").to_string();
            } else if line.starts_with("1 ") || line.starts_with("2 ") {
                // Changed entries
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 9 {
                    let xy = parts[1];
                    let path = parts[8];

                    let x = xy.chars().next().unwrap_or('.');
                    let y = xy.chars().nth(1).unwrap_or('.');

                    if x != '.' {
                        staged.push(format!("{} {}", Self::status_char(x), path));
                    }
                    if y != '.' {
                        unstaged.push(format!("{} {}", Self::status_char(y), path));
                    }
                }
            } else if line.starts_with("? ") {
                // Untracked
                let path = line.trim_start_matches("? ");
                untracked.push(path.to_string());
            }
        }

        let mut output = format!("Branch: {}\n", branch);

        if !staged.is_empty() {
            output.push_str("\nStaged:\n");
            for f in &staged {
                output.push_str(&format!("  {}\n", f));
            }
        }

        if !unstaged.is_empty() {
            output.push_str("\nUnstaged:\n");
            for f in &unstaged {
                output.push_str(&format!("  {}\n", f));
            }
        }

        if !untracked.is_empty() {
            output.push_str("\nUntracked:\n");
            for f in &untracked {
                output.push_str(&format!("  {}\n", f));
            }
        }

        if staged.is_empty() && unstaged.is_empty() && untracked.is_empty() {
            output.push_str("\nWorking tree clean");
        }

        output
    }

    fn status_char(c: char) -> &'static str {
        match c {
            'M' => "modified:",
            'A' => "added:",
            'D' => "deleted:",
            'R' => "renamed:",
            'C' => "copied:",
            'T' => "typechange:",
            'U' => "unmerged:",
            _ => "changed:",
        }
    }

    /// Get git diff (staged or unstaged changes)
    pub async fn git_diff(&self, args: &Value) -> Result<String> {
        let staged = args["staged"].as_bool().unwrap_or(false);
        let path = args["path"].as_str();

        let mut cmd = Command::new("git");
        cmd.current_dir(self.cwd);
        cmd.args(["diff", "--stat", "--patch"]);

        if staged {
            cmd.arg("--staged");
        }

        if let Some(p) = path {
            cmd.arg("--").arg(p);
        }

        let output = cmd.output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Ok(format!("Error: {}", stderr));
        }

        let diff = String::from_utf8_lossy(&output.stdout);
        if diff.is_empty() {
            return Ok("No changes".into());
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
    pub async fn git_commit(&self, args: &Value) -> Result<String> {
        let message = args["message"].as_str().unwrap_or("Update");
        let add_all = args["add_all"].as_bool().unwrap_or(false);

        // Optionally stage all changes
        if add_all {
            let add_output = Command::new("git")
                .current_dir(self.cwd)
                .args(["add", "-A"])
                .output()?;

            if !add_output.status.success() {
                let stderr = String::from_utf8_lossy(&add_output.stderr);
                return Ok(format!("Error staging: {}", stderr));
            }
        }

        // Check if there's anything to commit
        let status = Command::new("git")
            .current_dir(self.cwd)
            .args(["diff", "--staged", "--quiet"])
            .status()?;

        if status.success() {
            return Ok("Nothing staged to commit".into());
        }

        // Create commit
        let output = Command::new("git")
            .current_dir(self.cwd)
            .args(["commit", "-m", message])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Ok(format!("Error: {}", stderr));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout.lines().take(5).collect::<Vec<_>>().join("\n"))
    }

    /// Get recent commit log
    pub async fn git_log(&self, args: &Value) -> Result<String> {
        let limit = args["limit"].as_u64().unwrap_or(10) as usize;
        let path = args["path"].as_str();

        let mut cmd = Command::new("git");
        cmd.current_dir(self.cwd);
        cmd.args([
            "log",
            "--oneline",
            "--decorate",
            &format!("-{}", limit),
        ]);

        if let Some(p) = path {
            cmd.arg("--").arg(p);
        }

        let output = cmd.output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Ok(format!("Error: {}", stderr));
        }

        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}
