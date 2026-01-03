// crates/mira-server/src/web/claude.rs
// Claude Code PTY manager for spawning and managing Claude Code instances
// Instances are keyed by project path - one persistent instance per project

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

/// A running Claude Code instance tied to a project
pub struct ClaudeInstance {
    /// Unique instance ID (for WebSocket events)
    pub id: String,
    /// Project path this instance belongs to
    pub project_path: String,
    /// Working directory (same as project_path)
    pub working_dir: String,
    /// Child process
    child: Child,
    /// PTY master file descriptor
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

/// Info about a running Claude instance (for API/UI)
#[derive(Debug, Clone, serde::Serialize)]
pub struct ClaudeInstanceInfo {
    pub id: String,
    pub project_path: String,
    pub is_running: bool,
}

/// Manages Claude Code instances - one per project
pub struct ClaudeManager {
    /// Instances keyed by project path
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

    /// Get existing instance for project, or spawn a new one
    pub async fn get_or_spawn(&self, project_path: &str) -> Result<String> {
        // Check if instance exists and is running
        {
            let instances = self.instances.read().await;
            if let Some(instance) = instances.get(project_path) {
                let mut inst = instance.lock().await;
                if inst.is_running() {
                    info!("Reusing existing Claude instance {} for {}", inst.id, project_path);
                    return Ok(inst.id.clone());
                }
            }
        }

        // Spawn new interactive instance
        self.spawn_interactive(project_path.to_string()).await
    }

    /// Spawn a new interactive Claude Code instance (no --print, stays open)
    async fn spawn_interactive(&self, project_path: String) -> Result<String> {
        let id = Uuid::new_v4().to_string();

        // Open PTY
        let (master_fd, slave_fd) = open_pty()?;

        // Build command - interactive mode (no --print)
        let mut cmd = Command::new("claude");

        // Set working directory
        cmd.current_dir(&project_path);

        // Use PTY for stdin/stdout/stderr
        cmd.stdin(unsafe { Stdio::from_raw_fd(slave_fd.as_raw_fd()) });
        cmd.stdout(unsafe { Stdio::from_raw_fd(slave_fd.as_raw_fd()) });
        cmd.stderr(unsafe { Stdio::from_raw_fd(slave_fd.as_raw_fd()) });

        // Set environment for interactive mode
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLUMNS", "120");
        cmd.env("LINES", "40");

        // Spawn
        let child = cmd.spawn()?;

        info!("Spawned interactive Claude instance {} for project {}", id, project_path);

        let instance = ClaudeInstance {
            id: id.clone(),
            project_path: project_path.clone(),
            working_dir: project_path.clone(),
            child,
            master_fd,
        };

        // Store instance keyed by project path
        let instance = Arc::new(Mutex::new(instance));
        self.instances.write().await.insert(project_path.clone(), instance.clone());

        // Start output reader task
        let ws_tx = self.ws_tx.clone();
        let instance_id = id.clone();
        let instance_clone = instance.clone();
        let project_path_clone = project_path.clone();

        tokio::spawn(async move {
            loop {
                tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

                let mut inst = instance_clone.lock().await;

                // Check if still running
                if !inst.is_running() {
                    info!("Claude instance {} for project {} stopped", instance_id, project_path_clone);
                    let _ = ws_tx.send(WsEvent::ClaudeStopped {
                        instance_id: instance_id.clone(),
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

        // Broadcast spawn event with project info
        let _ = self.ws_tx.send(WsEvent::ClaudeSpawned {
            instance_id: id.clone(),
            working_dir: project_path,
        });

        Ok(id)
    }

    /// Send a task to the project's Claude instance (spawns if needed)
    pub async fn send_task(&self, project_path: &str, task: &str) -> Result<String> {
        let id = self.get_or_spawn(project_path).await?;

        // Send the task
        let instances = self.instances.read().await;
        if let Some(instance) = instances.get(project_path) {
            let mut inst = instance.lock().await;
            inst.write(task)?;
            info!("Sent task to Claude {} for project {}", id, project_path);
            Ok(id)
        } else {
            Err(anyhow!("Instance not found after spawn"))
        }
    }

    /// Close the Claude instance for a project
    pub async fn close_project(&self, project_path: &str) -> Result<()> {
        let mut instances = self.instances.write().await;

        if let Some(instance) = instances.remove(project_path) {
            let mut inst = instance.lock().await;
            let id = inst.id.clone();
            inst.kill()?;
            info!("Closed Claude instance {} for project {}", id, project_path);

            let _ = self.ws_tx.send(WsEvent::ClaudeStopped {
                instance_id: id,
            });
        }

        Ok(())
    }

    /// Check if a project has a running Claude instance
    pub async fn has_instance(&self, project_path: &str) -> bool {
        if let Some(instance) = self.instances.read().await.get(project_path) {
            instance.lock().await.is_running()
        } else {
            false
        }
    }

    /// Get instance ID for a project (if exists and running)
    pub async fn get_instance_id(&self, project_path: &str) -> Option<String> {
        if let Some(instance) = self.instances.read().await.get(project_path) {
            let mut inst = instance.lock().await;
            if inst.is_running() {
                return Some(inst.id.clone());
            }
        }
        None
    }

    /// List all running instances with their project info
    pub async fn list_all(&self) -> Vec<ClaudeInstanceInfo> {
        let instances = self.instances.read().await;
        let mut result = Vec::new();

        for (project_path, instance) in instances.iter() {
            let mut inst = instance.lock().await;
            result.push(ClaudeInstanceInfo {
                id: inst.id.clone(),
                project_path: project_path.clone(),
                is_running: inst.is_running(),
            });
        }

        result
    }

    // ═══════════════════════════════════════
    // LEGACY METHODS (for backwards compatibility during transition)
    // ═══════════════════════════════════════

    /// Send input to a running instance by ID
    pub async fn send_input(&self, instance_id: &str, input: &str) -> Result<()> {
        let instances = self.instances.read().await;

        // Find instance by ID (search through all)
        for instance in instances.values() {
            let mut inst = instance.lock().await;
            if inst.id == instance_id {
                inst.write(input)?;
                debug!("Sent to Claude {}: {}", instance_id, input);
                return Ok(());
            }
        }

        Err(anyhow!("Instance {} not found", instance_id))
    }

    /// Kill a running instance by ID
    pub async fn kill(&self, instance_id: &str) -> Result<()> {
        let mut instances = self.instances.write().await;

        // Find and remove instance by ID
        let mut project_to_remove = None;
        for (project_path, instance) in instances.iter() {
            let inst = instance.lock().await;
            if inst.id == instance_id {
                project_to_remove = Some(project_path.clone());
                break;
            }
        }

        if let Some(project_path) = project_to_remove {
            if let Some(instance) = instances.remove(&project_path) {
                let mut inst = instance.lock().await;
                inst.kill()?;
                info!("Killed Claude instance {}", instance_id);

                let _ = self.ws_tx.send(WsEvent::ClaudeStopped {
                    instance_id: instance_id.to_string(),
                });
            }
        }

        Ok(())
    }

    /// Spawn with --print mode (legacy, for discuss tool)
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
        cmd.current_dir(&working_dir);

        cmd.stdin(unsafe { Stdio::from_raw_fd(slave_fd.as_raw_fd()) });
        cmd.stdout(unsafe { Stdio::from_raw_fd(slave_fd.as_raw_fd()) });
        cmd.stderr(unsafe { Stdio::from_raw_fd(slave_fd.as_raw_fd()) });

        // Add initial prompt with --print mode
        if let Some(prompt) = &initial_prompt {
            cmd.args(["--print", prompt]);
        }

        cmd.env("TERM", "xterm-256color");
        cmd.env("COLUMNS", "120");
        cmd.env("LINES", "40");

        let child = cmd.spawn()?;

        info!("Spawned Claude Code instance {} (legacy --print mode) in {}", id, working_dir);

        let instance = ClaudeInstance {
            id: id.clone(),
            project_path: working_dir.clone(),
            working_dir: working_dir.clone(),
            child,
            master_fd,
        };

        // Store by ID for legacy compatibility (not project path)
        let instance = Arc::new(Mutex::new(instance));
        // Use ID as key for legacy spawn (not project path)
        self.instances.write().await.insert(id.clone(), instance.clone());

        // Start output reader task
        let ws_tx = self.ws_tx.clone();
        let instance_id = id.clone();
        let instance_clone = instance.clone();

        tokio::spawn(async move {
            loop {
                tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

                let mut inst = instance_clone.lock().await;

                if !inst.is_running() {
                    info!("Claude instance {} stopped", instance_id);
                    let _ = ws_tx.send(WsEvent::ClaudeStopped {
                        instance_id: instance_id.clone(),
                    });
                    break;
                }

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

        Ok(id)
    }
}
