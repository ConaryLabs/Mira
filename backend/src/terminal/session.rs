// backend/src/terminal/session.rs

use super::types::*;
use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};
use std::io::{Read, Write};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

/// Manages an interactive terminal session with PTY
pub struct TerminalSession {
    session_id: String,
    project_id: String,
    config: TerminalConfig,
    pty_system: NativePtySystem,
}

impl TerminalSession {
    /// Create a new terminal session
    pub fn new(config: TerminalConfig) -> Self {
        let session_id = uuid::Uuid::new_v4().to_string();
        let project_id = config.project_id.clone();

        Self {
            session_id,
            project_id,
            config,
            pty_system: NativePtySystem::default(),
        }
    }

    /// Start an interactive shell session
    /// Returns channels for bidirectional communication
    pub async fn start_shell(
        &self,
    ) -> TerminalResult<(mpsc::Sender<TerminalMessage>, mpsc::Receiver<TerminalMessage>)> {
        info!("Starting shell session {}", self.session_id);

        // Create PTY pair
        let pty_size = PtySize {
            rows: self.config.rows,
            cols: self.config.cols,
            pixel_width: 0,
            pixel_height: 0,
        };

        let pair = self
            .pty_system
            .openpty(pty_size)
            .map_err(|e| TerminalError::TerminalError(format!("Failed to open PTY: {}", e)))?;

        // Determine shell to use
        let shell = self
            .config
            .shell
            .clone()
            .unwrap_or_else(|| Self::get_default_shell());

        // Create command
        let mut cmd = CommandBuilder::new(&shell);

        if let Some(ref cwd) = self.config.working_directory {
            cmd.cwd(cwd);
        }

        for (key, value) in &self.config.environment {
            cmd.env(key, value);
        }

        // Spawn child process
        let mut child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| TerminalError::TerminalError(format!("Failed to spawn shell: {}", e)))?;

        info!("Shell spawned for session {}", self.session_id);

        // Get reader and writer from master PTY
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| TerminalError::TerminalError(format!("Failed to clone reader: {}", e)))?;

        let mut writer = pair
            .master
            .take_writer()
            .map_err(|e| TerminalError::TerminalError(format!("Failed to take writer: {}", e)))?;

        // Create channels for bidirectional communication
        let (input_tx, mut input_rx) = mpsc::channel::<TerminalMessage>(100);
        let (output_tx, output_rx) = mpsc::channel::<TerminalMessage>(100);

        // Clone for tasks
        let session_id = self.session_id.clone();

        // Spawn task to handle input from frontend
        tokio::task::spawn_blocking(move || {
            while let Some(msg) = input_rx.blocking_recv() {
                match msg {
                    TerminalMessage::Input { data } => {
                        debug!("Received {} bytes from frontend", data.len());

                        if let Err(e) = writer.write_all(&data) {
                            error!("Failed to write to PTY: {}", e);
                            break;
                        }
                        if let Err(e) = writer.flush() {
                            error!("Failed to flush PTY: {}", e);
                            break;
                        }
                    }
                    TerminalMessage::Resize { cols, rows } => {
                        debug!("Resizing terminal to {}x{}", cols, rows);
                        // Resize is handled separately if needed
                    }
                    TerminalMessage::Closed { .. } => {
                        debug!("Client requested close");
                        break;
                    }
                    _ => {
                        warn!("Unexpected message type from frontend");
                    }
                }
            }

            debug!("Input handler stopped for session {}", session_id);
        });

        // Spawn task to read output from PTY
        let session_id_clone = self.session_id.clone();
        tokio::task::spawn_blocking(move || {
            let mut buffer = [0u8; 4096];

            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => {
                        debug!("PTY EOF reached");
                        let _ = output_tx.blocking_send(TerminalMessage::Closed {
                            exit_code: None,
                        });
                        break;
                    }
                    Ok(n) => {
                        debug!("Read {} bytes from PTY", n);

                        let output = TerminalMessage::Output {
                            data: buffer[..n].to_vec(),
                        };

                        if output_tx.blocking_send(output).is_err() {
                            debug!("Output channel closed, stopping PTY reader");
                            break;
                        }
                    }
                    Err(e) => {
                        if e.kind() != std::io::ErrorKind::WouldBlock {
                            error!("Error reading from PTY: {}", e);
                            let _ = output_tx.blocking_send(TerminalMessage::Error {
                                message: format!("PTY read error: {}", e),
                            });
                            break;
                        }
                    }
                }
            }

            debug!("Output handler stopped for session {}", session_id_clone);
        });

        // Spawn task to wait for child process
        tokio::task::spawn_blocking(move || {
            match child.wait() {
                Ok(status) => {
                    info!("Shell exited with status: {:?}", status);
                }
                Err(e) => {
                    error!("Error waiting for shell: {}", e);
                }
            }
        });

        Ok((input_tx, output_rx))
    }

    /// Get the session ID
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Get the project ID
    pub fn project_id(&self) -> &str {
        &self.project_id
    }

    /// Get default system shell
    fn get_default_shell() -> String {
        #[cfg(unix)]
        {
            std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string())
        }

        #[cfg(windows)]
        {
            std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string())
        }
    }
}
