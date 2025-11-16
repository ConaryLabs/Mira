// backend/src/terminal/process_executor.rs

use super::types::*;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use tracing::{debug, info};

/// Executes commands and processes on the local machine
pub struct ProcessExecutor {
    /// Default working directory for commands
    working_directory: Option<PathBuf>,
}

impl ProcessExecutor {
    /// Create a new process executor
    pub fn new(working_directory: Option<PathBuf>) -> Self {
        Self { working_directory }
    }

    /// Execute a command and wait for completion
    pub async fn execute(
        &self,
        command: &str,
        args: &[&str],
        working_dir: Option<&PathBuf>,
    ) -> TerminalResult<CommandResult> {
        info!("Executing command: {} {:?}", command, args);

        let start = std::time::Instant::now();

        let dir = working_dir
            .or(self.working_directory.as_ref())
            .cloned()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        let output = Command::new(command)
            .args(args)
            .current_dir(&dir)
            .output()
            .await
            .map_err(|e| TerminalError::CommandFailed(format!("Failed to execute command: {}", e)))?;

        let duration = start.elapsed();

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let exit_code = output.status.code().unwrap_or(-1);

        debug!(
            "Command completed in {:?}ms with exit code {}",
            duration.as_millis(),
            exit_code
        );

        Ok(CommandResult {
            stdout,
            stderr,
            exit_code,
            duration_ms: duration.as_millis() as u64,
        })
    }

    /// Execute a shell command (uses sh on Unix, cmd on Windows)
    pub async fn execute_shell(&self, command: &str, working_dir: Option<&PathBuf>) -> TerminalResult<CommandResult> {
        info!("Executing shell command: {}", command);

        #[cfg(unix)]
        let (shell, flag) = ("sh", "-c");

        #[cfg(windows)]
        let (shell, flag) = ("cmd", "/C");

        self.execute(shell, &[flag, command], working_dir).await
    }

    /// Execute a command with streaming output
    pub async fn execute_streaming(
        &self,
        command: &str,
        args: &[&str],
        working_dir: Option<&PathBuf>,
    ) -> TerminalResult<mpsc::Receiver<TerminalMessage>> {
        info!("Executing command (streaming): {} {:?}", command, args);

        let dir = working_dir
            .or(self.working_directory.as_ref())
            .cloned()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        let mut child = Command::new(command)
            .args(args)
            .current_dir(&dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| TerminalError::CommandFailed(format!("Failed to spawn command: {}", e)))?;

        let stdout = child.stdout.take().ok_or_else(|| {
            TerminalError::CommandFailed("Failed to capture stdout".to_string())
        })?;

        let stderr = child.stderr.take().ok_or_else(|| {
            TerminalError::CommandFailed("Failed to capture stderr".to_string())
        })?;

        let (output_tx, output_rx) = mpsc::channel::<TerminalMessage>(100);

        // Spawn task to stream stdout
        let stdout_tx = output_tx.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();

            while let Ok(n) = reader.read_line(&mut line).await {
                if n == 0 {
                    break;
                }

                let _ = stdout_tx
                    .send(TerminalMessage::Output {
                        data: line.as_bytes().to_vec(),
                    })
                    .await;

                line.clear();
            }
        });

        // Spawn task to stream stderr
        let stderr_tx = output_tx.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(stderr);
            let mut line = String::new();

            while let Ok(n) = reader.read_line(&mut line).await {
                if n == 0 {
                    break;
                }

                let _ = stderr_tx
                    .send(TerminalMessage::Output {
                        data: line.as_bytes().to_vec(),
                    })
                    .await;

                line.clear();
            }
        });

        // Spawn task to wait for process completion
        tokio::spawn(async move {
            match child.wait().await {
                Ok(status) => {
                    let exit_code = status.code();
                    let _ = output_tx
                        .send(TerminalMessage::Closed { exit_code })
                        .await;
                }
                Err(e) => {
                    let _ = output_tx
                        .send(TerminalMessage::Error {
                            message: format!("Process error: {}", e),
                        })
                        .await;
                }
            }
        });

        Ok(output_rx)
    }

    /// Execute a shell command with streaming output
    pub async fn execute_shell_streaming(
        &self,
        command: &str,
        working_dir: Option<&PathBuf>,
    ) -> TerminalResult<mpsc::Receiver<TerminalMessage>> {
        info!("Executing shell command (streaming): {}", command);

        #[cfg(unix)]
        let (shell, flag) = ("sh", "-c");

        #[cfg(windows)]
        let (shell, flag) = ("cmd", "/C");

        self.execute_streaming(shell, &[flag, command], working_dir).await
    }

    /// Execute multiple commands in sequence
    pub async fn execute_batch(
        &self,
        commands: Vec<(String, Vec<String>)>,
        working_dir: Option<&PathBuf>,
    ) -> TerminalResult<Vec<CommandResult>> {
        info!("Executing batch of {} commands", commands.len());

        let mut results = Vec::with_capacity(commands.len());

        for (command, args) in commands {
            let args_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            let result = self.execute(&command, &args_refs, working_dir).await?;

            // Stop on first error
            let should_stop = result.exit_code != 0;
            let exit_code = result.exit_code;

            results.push(result);

            if should_stop {
                debug!("Command failed with exit code {}, stopping batch", exit_code);
                break;
            }
        }

        Ok(results)
    }

    /// Get system shell path
    pub fn get_shell() -> String {
        #[cfg(unix)]
        {
            std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())
        }

        #[cfg(windows)]
        {
            std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string())
        }
    }
}

impl Default for ProcessExecutor {
    fn default() -> Self {
        Self::new(None)
    }
}
