// src/web/claude.rs
// Claude Code PTY manager for spawning and managing Claude Code instances

use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::io::Write;
use std::os::unix::io::{AsRawFd, FromRawFd, OwnedFd};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex, RwLock};
use tracing::{debug, info, warn};
use uuid::Uuid;

use mira_types::WsEvent;

// ═══════════════════════════════════════
// PTY UTILITIES
// ═══════════════════════════════════════

/// Open a new pseudo-terminal pair
fn open_pty() -> Result<(OwnedFd, OwnedFd)> {
    unsafe {
        // Open master
        let master_fd = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY);
        if master_fd < 0 {
            return Err(anyhow!("posix_openpt failed"));
        }

        // Grant access
        if libc::grantpt(master_fd) < 0 {
            libc::close(master_fd);
            return Err(anyhow!("grantpt failed"));
        }

        // Unlock
        if libc::unlockpt(master_fd) < 0 {
            libc::close(master_fd);
            return Err(anyhow!("unlockpt failed"));
        }

        // Get slave name
        let slave_name = libc::ptsname(master_fd);
        if slave_name.is_null() {
            libc::close(master_fd);
            return Err(anyhow!("ptsname failed"));
        }

        // Open slave
        let slave_fd = libc::open(slave_name, libc::O_RDWR | libc::O_NOCTTY);
        if slave_fd < 0 {
            libc::close(master_fd);
            return Err(anyhow!("Failed to open slave PTY"));
        }

        Ok((
            OwnedFd::from_raw_fd(master_fd),
            OwnedFd::from_raw_fd(slave_fd),
        ))
    }
}

// ═══════════════════════════════════════
// CLAUDE INSTANCE
// ═══════════════════════════════════════

/// A running Claude Code instance
pub struct ClaudeInstance {
    pub id: String,
    pub working_dir: String,
    child: Child,
    master_fd: OwnedFd,
}

impl ClaudeInstance {
    /// Write to the instance's stdin
    pub fn write(&mut self, data: &str) -> Result<()> {
        let mut file = unsafe { std::fs::File::from_raw_fd(self.master_fd.as_raw_fd()) };
        file.write_all(data.as_bytes())?;
        file.write_all(b"\n")?;
        file.flush()?;
        // Don't close the fd - we still need it
        std::mem::forget(file);
        Ok(())
    }

    /// Read available output (non-blocking)
    pub fn read(&self) -> Result<Option<String>> {
        use std::os::unix::io::AsRawFd;

        let fd = self.master_fd.as_raw_fd();

        // Check if data is available
        let mut poll_fds = [libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        }];

        let ret = unsafe { libc::poll(poll_fds.as_mut_ptr(), 1, 0) };

        if ret <= 0 || (poll_fds[0].revents & libc::POLLIN) == 0 {
            return Ok(None);
        }

        // Read available data
        let mut buf = [0u8; 4096];
        let n = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };

        if n <= 0 {
            return Ok(None);
        }

        let output = String::from_utf8_lossy(&buf[..n as usize]).to_string();
        Ok(Some(output))
    }

    /// Check if the process is still running
    pub fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Kill the instance
    pub fn kill(&mut self) -> Result<()> {
        self.child.kill()?;
        Ok(())
    }
}

// ═══════════════════════════════════════
// MANAGER
// ═══════════════════════════════════════

/// Manages Claude Code instances
pub struct ClaudeManager {
    instances: RwLock<HashMap<String, Arc<Mutex<ClaudeInstance>>>>,
    ws_tx: broadcast::Sender<WsEvent>,
}

impl ClaudeManager {
    /// Create a new Claude manager
    pub fn new(ws_tx: broadcast::Sender<WsEvent>) -> Self {
        Self {
            instances: RwLock::new(HashMap::new()),
            ws_tx,
        }
    }

    /// Spawn a new Claude Code instance
    pub async fn spawn(
        &self,
        working_dir: String,
        initial_prompt: Option<String>,
    ) -> Result<String> {
        let id = Uuid::new_v4().to_string();

        // Open PTY
        let (master_fd, slave_fd) = open_pty()?;

        // Build command
        let mut cmd = Command::new("claude");

        // Set working directory
        cmd.current_dir(&working_dir);

        // Use PTY for stdin/stdout/stderr
        cmd.stdin(unsafe { Stdio::from_raw_fd(slave_fd.as_raw_fd()) });
        cmd.stdout(unsafe { Stdio::from_raw_fd(slave_fd.as_raw_fd()) });
        cmd.stderr(unsafe { Stdio::from_raw_fd(slave_fd.as_raw_fd()) });

        // Add initial prompt if provided
        if let Some(prompt) = &initial_prompt {
            cmd.args(["--print", prompt]);
        }

        // Set environment for non-interactive mode
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLUMNS", "120");
        cmd.env("LINES", "40");

        // Spawn
        let child = cmd.spawn()?;

        info!("Spawned Claude Code instance {} in {}", id, working_dir);

        let instance = ClaudeInstance {
            id: id.clone(),
            working_dir,
            child,
            master_fd,
        };

        // Store instance
        let instance = Arc::new(Mutex::new(instance));
        self.instances.write().await.insert(id.clone(), instance.clone());

        // Start output reader task
        let ws_tx = self.ws_tx.clone();
        let instance_id = id.clone();
        let instance_clone = instance.clone();

        tokio::spawn(async move {
            loop {
                tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

                let mut inst = instance_clone.lock().await;

                // Check if still running
                if !inst.is_running() {
                    info!("Claude instance {} stopped", instance_id);
                    let _ = ws_tx.send(WsEvent::TerminalOutput {
                        instance_id: instance_id.clone(),
                        content: format!("\n[Claude instance {} stopped]\n", instance_id),
                        is_stderr: true,
                    });
                    break;
                }

                // Read output
                match inst.read() {
                    Ok(Some(output)) => {
                        debug!("Claude {} output: {}", instance_id, output);
                        let _ = ws_tx.send(WsEvent::TerminalOutput {
                            instance_id: instance_id.clone(),
                            content: output,
                            is_stderr: false,
                        });
                    }
                    Ok(None) => {}
                    Err(e) => {
                        warn!("Error reading from Claude {}: {}", instance_id, e);
                        break;
                    }
                }
            }
        });

        // Broadcast spawn event
        let _ = self.ws_tx.send(WsEvent::TerminalOutput {
            instance_id: id.clone(),
            content: format!("[Started Claude Code instance {}]\n", id),
            is_stderr: false,
        });

        Ok(id)
    }

    /// Send input to a running instance
    pub async fn send_input(&self, instance_id: &str, input: &str) -> Result<()> {
        let instances = self.instances.read().await;
        let instance = instances
            .get(instance_id)
            .ok_or_else(|| anyhow!("Instance {} not found", instance_id))?;

        let mut inst = instance.lock().await;
        inst.write(input)?;

        debug!("Sent to Claude {}: {}", instance_id, input);
        Ok(())
    }

    /// Kill a running instance
    pub async fn kill(&self, instance_id: &str) -> Result<()> {
        let mut instances = self.instances.write().await;

        if let Some(instance) = instances.remove(instance_id) {
            let mut inst = instance.lock().await;
            inst.kill()?;
            info!("Killed Claude instance {}", instance_id);

            let _ = self.ws_tx.send(WsEvent::TerminalOutput {
                instance_id: instance_id.to_string(),
                content: format!("[Claude instance {} killed]\n", instance_id),
                is_stderr: true,
            });
        }

        Ok(())
    }

    /// List running instances
    pub async fn list(&self) -> Vec<String> {
        self.instances.read().await.keys().cloned().collect()
    }

    /// Check if an instance exists and is running
    pub async fn is_running(&self, instance_id: &str) -> bool {
        if let Some(instance) = self.instances.read().await.get(instance_id) {
            instance.lock().await.is_running()
        } else {
            false
        }
    }
}
