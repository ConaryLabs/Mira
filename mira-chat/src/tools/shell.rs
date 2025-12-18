//! Shell execution tool

use anyhow::Result;
use serde_json::Value;
use std::path::Path;

/// Maximum output size (8KB) - keeps token usage sane
const MAX_OUTPUT_SIZE: usize = 8 * 1024;

/// Shell tool implementations
pub struct ShellTools<'a> {
    pub cwd: &'a Path,
}

impl<'a> ShellTools<'a> {
    /// Truncate output to MAX_OUTPUT_SIZE with a helpful message
    fn truncate_output(output: &str) -> String {
        if output.len() <= MAX_OUTPUT_SIZE {
            return output.to_string();
        }

        // Keep first ~75% and last ~20% to show both start and end
        let head_size = (MAX_OUTPUT_SIZE * 3) / 4;
        let tail_size = MAX_OUTPUT_SIZE / 5;
        let head: String = output.chars().take(head_size).collect();
        let tail: String = output.chars().rev().take(tail_size).collect::<String>().chars().rev().collect();

        let omitted = output.len() - head_size - tail_size;
        format!(
            "{}\n\n... [{} bytes omitted - use head/tail/grep for specific output] ...\n\n{}",
            head, omitted, tail
        )
    }

    pub async fn bash(&self, args: &Value) -> Result<String> {
        let command = args["command"].as_str().unwrap_or("");

        let output = tokio::process::Command::new("bash")
            .args(["-c", command])
            .current_dir(self.cwd)
            .output()
            .await?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let exit_code = output.status.code().unwrap_or(-1);
        let cwd_display = self.cwd.display();

        // Always include metadata header for debugging
        let mut result = String::new();

        // Compact metadata line
        result.push_str(&format!("$ {} [exit={}, cwd={}]\n", command, exit_code, cwd_display));

        // Output content
        if !stdout.is_empty() {
            result.push_str(&stdout);
        }

        // Stderr (if any)
        if !stderr.is_empty() {
            if !stdout.is_empty() && !stdout.ends_with('\n') {
                result.push('\n');
            }
            result.push_str("[stderr]\n");
            result.push_str(&stderr);
        }

        Ok(Self::truncate_output(&result))
    }
}
