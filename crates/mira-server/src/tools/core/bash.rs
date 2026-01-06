//! Unified bash tool

use std::time::Duration;
use tokio::process::Command;
use tracing::{info, warn, error};

use crate::tools::core::ToolContext;

/// Execute bash command
/// Works in web chat; in MCP returns web-only error
pub async fn bash<C: ToolContext>(
    _ctx: &C,
    command: String,
    working_directory: Option<String>,
    timeout_seconds: Option<u64>,
) -> Result<String, String> {
    if command.is_empty() {
        return Err("command is required".to_string());
    }

    let timeout = timeout_seconds.unwrap_or(60);

    let mut cmd = Command::new("bash");
    cmd.arg("-c").arg(&command);

    if let Some(dir) = &working_directory {
        cmd.current_dir(dir);
    }

    info!(command = %command, working_dir = ?working_directory, timeout = timeout, "Executing bash command");

    match tokio::time::timeout(Duration::from_secs(timeout), cmd.output()).await {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let exit_code = output.status.code().unwrap_or(-1);

            let result = if stderr.is_empty() {
                format!("Exit: {}\n{}", exit_code, stdout)
            } else if stdout.is_empty() {
                format!("Exit: {}\n{}", exit_code, stderr)
            } else {
                format!("Exit: {}\nstdout:\n{}\nstderr:\n{}", exit_code, stdout, stderr)
            };

            if exit_code == 0 {
                info!(exit_code = exit_code, "Bash command completed successfully");
            } else {
                warn!(exit_code = exit_code, "Bash command exited with non-zero status");
            }

            Ok(result)
        }
        Ok(Err(e)) => {
            error!(error = %e, "Failed to execute bash command");
            Err(format!("Failed to execute: {}", e))
        }
        Err(_) => {
            warn!(timeout = timeout, "Bash command timed out");
            Err(format!("Command timed out after {}s", timeout))
        }
    }
}
