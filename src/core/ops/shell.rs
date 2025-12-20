//! Core shell operations - shared by MCP and Chat
//!
//! Pure implementation of bash execution.

use std::path::PathBuf;
use std::time::Duration;

use super::super::{CoreError, CoreResult};

/// Maximum output size (64KB)
const MAX_OUTPUT_SIZE: usize = 64 * 1024;

// ============================================================================
// Input/Output Types
// ============================================================================

#[derive(Clone)]
pub struct BashInput {
    pub command: String,
    pub cwd: PathBuf,
    pub timeout: Option<Duration>,
}

pub struct BashOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub success: bool,
    pub truncated: bool,
}

// ============================================================================
// Operations
// ============================================================================

/// Execute a bash command
pub async fn bash(input: BashInput) -> CoreResult<BashOutput> {
    let timeout = input.timeout.unwrap_or(Duration::from_secs(120));

    let result = tokio::time::timeout(
        timeout,
        tokio::process::Command::new("bash")
            .args(["-c", &input.command])
            .current_dir(&input.cwd)
            .output()
    ).await;

    let output = match result {
        Ok(Ok(output)) => output,
        Ok(Err(e)) => {
            return Err(CoreError::ShellExec(input.command, e.to_string()));
        }
        Err(_) => {
            return Err(CoreError::ShellTimeout(input.command, timeout.as_secs()));
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let exit_code = output.status.code().unwrap_or(-1);

    // Check if output needs truncation
    let total_len = stdout.len() + stderr.len();
    let truncated = total_len > MAX_OUTPUT_SIZE;

    let (stdout, stderr) = if truncated {
        truncate_output(&stdout, &stderr, MAX_OUTPUT_SIZE)
    } else {
        (stdout, stderr)
    };

    Ok(BashOutput {
        stdout,
        stderr,
        exit_code,
        success: output.status.success(),
        truncated,
    })
}

/// Truncate output keeping beginning and end for context
fn truncate_output(stdout: &str, stderr: &str, max_size: usize) -> (String, String) {
    let total = stdout.len() + stderr.len();
    if total <= max_size {
        return (stdout.to_string(), stderr.to_string());
    }

    // Allocate proportionally
    let stdout_ratio = stdout.len() as f64 / total as f64;
    let stdout_budget = (max_size as f64 * stdout_ratio) as usize;
    let stderr_budget = max_size - stdout_budget;

    let truncated_stdout = truncate_single(stdout, stdout_budget);
    let truncated_stderr = truncate_single(stderr, stderr_budget);

    (truncated_stdout, truncated_stderr)
}

/// Truncate a single string keeping head and tail
fn truncate_single(s: &str, max_size: usize) -> String {
    if s.len() <= max_size {
        return s.to_string();
    }

    // Keep first ~75% and last ~20%
    let head_size = (max_size * 3) / 4;
    let tail_size = max_size / 5;

    let head: String = s.chars().take(head_size).collect();
    let tail: String = s.chars().rev().take(tail_size).collect::<String>().chars().rev().collect();

    let omitted = s.len() - head_size - tail_size;
    format!(
        "{}\n\n... [{} bytes omitted] ...\n\n{}",
        head, omitted, tail
    )
}

/// Format bash output for display
pub fn format_output(input: &BashInput, output: &BashOutput) -> String {
    let mut result = String::new();

    // Compact metadata line
    result.push_str(&format!(
        "$ {} [exit={}, cwd={}]\n",
        input.command,
        output.exit_code,
        input.cwd.display()
    ));

    // Stdout
    if !output.stdout.is_empty() {
        result.push_str(&output.stdout);
    }

    // Stderr
    if !output.stderr.is_empty() {
        if !output.stdout.is_empty() && !output.stdout.ends_with('\n') {
            result.push('\n');
        }
        result.push_str("[stderr]\n");
        result.push_str(&output.stderr);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_bash_echo() {
        let output = bash(BashInput {
            command: "echo hello".to_string(),
            cwd: PathBuf::from("/tmp"),
            timeout: None,
        }).await.unwrap();

        assert!(output.success);
        assert_eq!(output.exit_code, 0);
        assert_eq!(output.stdout.trim(), "hello");
        assert!(output.stderr.is_empty());
    }

    #[tokio::test]
    async fn test_bash_exit_code() {
        let output = bash(BashInput {
            command: "exit 42".to_string(),
            cwd: PathBuf::from("/tmp"),
            timeout: None,
        }).await.unwrap();

        assert!(!output.success);
        assert_eq!(output.exit_code, 42);
    }

    #[test]
    fn test_truncate() {
        let long_string = "x".repeat(100);
        let truncated = truncate_single(&long_string, 50);
        assert!(truncated.len() < 100);
        assert!(truncated.contains("bytes omitted"));
    }
}
