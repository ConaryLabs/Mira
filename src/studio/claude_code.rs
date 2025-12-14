// src/studio/claude_code.rs
// Claude Code process spawning and output streaming

use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tracing::{info, error};

use super::types::{StudioState, WorkspaceEvent};

/// Launch Claude Code with a task and stream output to workspace events
pub async fn launch_claude_code(
    state: StudioState,
    task: String,
    project_path: Option<String>,
) {
    // Emit start event
    state.emit(WorkspaceEvent::ClaudeCodeStart { task: task.clone() });

    info!("Launching Claude Code with task: {}", &task[..task.len().min(100)]);

    // Build the command - use full path since systemd has minimal PATH
    let claude_path = std::env::var("CLAUDE_PATH")
        .unwrap_or_else(|_| "/home/peter/.local/bin/claude".to_string());

    let mut cmd = Command::new(&claude_path);
    cmd.arg("-p").arg(&task);
    cmd.arg("--print");

    // Set working directory if provided
    if let Some(path) = project_path {
        cmd.current_dir(&path);
    }

    // Capture stdout and stderr
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    // Spawn the process
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            error!("Failed to spawn Claude Code: {}", e);
            state.emit(WorkspaceEvent::ClaudeCodeOutput {
                line: format!("Error: Failed to launch Claude Code: {}", e),
                stream: "stderr".to_string(),
            });
            state.emit(WorkspaceEvent::ClaudeCodeEnd {
                exit_code: -1,
                success: false,
            });
            return;
        }
    };

    // Take stdout and stderr handles
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let state_stdout = state.clone();
    let state_stderr = state.clone();

    // Spawn task to read stdout
    let stdout_handle = tokio::spawn(async move {
        if let Some(stdout) = stdout {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();

            while let Ok(Some(line)) = lines.next_line().await {
                state_stdout.emit(WorkspaceEvent::ClaudeCodeOutput {
                    line,
                    stream: "stdout".to_string(),
                });
            }
        }
    });

    // Spawn task to read stderr
    let stderr_handle = tokio::spawn(async move {
        if let Some(stderr) = stderr {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();

            while let Ok(Some(line)) = lines.next_line().await {
                state_stderr.emit(WorkspaceEvent::ClaudeCodeOutput {
                    line,
                    stream: "stderr".to_string(),
                });
            }
        }
    });

    // Wait for both readers to complete
    let _ = tokio::join!(stdout_handle, stderr_handle);

    // Wait for the process to complete
    let exit_code = match child.wait().await {
        Ok(status) => status.code().unwrap_or(-1),
        Err(e) => {
            error!("Failed to wait for Claude Code: {}", e);
            -1
        }
    };

    let success = exit_code == 0;
    info!("Claude Code finished with exit code: {} (success: {})", exit_code, success);

    state.emit(WorkspaceEvent::ClaudeCodeEnd { exit_code, success });
}
