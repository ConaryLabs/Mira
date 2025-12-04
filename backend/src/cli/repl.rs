// backend/src/cli/repl.rs
// Interactive REPL loop for Mira CLI

use anyhow::{Context, Result};
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::cli::args::{CliArgs, OutputFormat};
use crate::cli::config::CliConfig;
use crate::cli::display::{StreamingDisplay, TerminalDisplay};
use crate::cli::project::{build_metadata, ProjectDetector, DetectedProject};
use crate::cli::session::{simple_session_list, CliSession, SessionPicker, SessionStore, SessionFilter};
use crate::cli::ws_client::{BackendEvent, MiraClient};

/// REPL state
pub struct Repl {
    /// CLI arguments
    args: CliArgs,
    /// CLI configuration
    config: CliConfig,
    /// WebSocket client
    client: MiraClient,
    /// Streaming display
    display: StreamingDisplay,
    /// Line editor
    editor: DefaultEditor,
    /// Interrupt flag
    interrupted: Arc<AtomicBool>,
    /// Running flag
    running: bool,
    /// Session store
    session_store: SessionStore,
    /// Current session
    current_session: Option<CliSession>,
    /// Detected project context
    project: Option<DetectedProject>,
}

impl Repl {
    /// Create a new REPL instance
    pub async fn new(args: CliArgs) -> Result<Self> {
        // Load config
        let config = CliConfig::load().unwrap_or_default();

        // Ensure directories exist
        CliConfig::ensure_dirs()?;

        // Initialize session store
        let session_store = SessionStore::new().await
            .context("Failed to initialize session store")?;

        // Detect project context
        let project = if let Some(ref project_path) = args.project {
            ProjectDetector::detect_from(project_path)?
        } else {
            ProjectDetector::detect()?
        };

        // Connect to backend
        let backend_url = if args.backend_url != "ws://localhost:3001/ws" {
            args.backend_url.clone()
        } else {
            config.backend_url.clone()
        };

        let client = MiraClient::connect(&backend_url)
            .await
            .with_context(|| format!("Failed to connect to backend at {}", backend_url))?;

        // Create display
        let terminal = TerminalDisplay::new(
            args.no_color,
            args.verbose || config.verbose,
            args.show_thinking || config.show_thinking,
        );
        let display = StreamingDisplay::new(terminal, args.output_format);

        // Create line editor
        let editor = DefaultEditor::new()
            .context("Failed to create line editor")?;

        // Setup interrupt handler
        let interrupted = Arc::new(AtomicBool::new(false));
        let interrupted_clone = interrupted.clone();
        ctrlc::set_handler(move || {
            interrupted_clone.store(true, Ordering::SeqCst);
        })
        .context("Failed to set Ctrl+C handler")?;

        Ok(Self {
            args,
            config,
            client,
            display,
            editor,
            interrupted,
            running: true,
            session_store,
            current_session: None,
            project,
        })
    }

    /// Run the REPL
    pub async fn run(&mut self) -> Result<()> {
        // Check for one-shot mode
        if self.args.is_non_interactive() {
            return self.run_one_shot().await;
        }

        // Handle session resume flags
        if self.args.should_show_picker() {
            // Show session picker
            self.show_session_picker().await?;
        } else if self.args.continue_session {
            // Continue most recent session
            self.continue_recent_session().await?;
        } else if let Some(session_id) = self.args.get_resume_session_id().map(|s| s.to_string()) {
            // Resume specific session
            self.resume_session(&session_id).await?;
        } else {
            // Create new session
            self.create_new_session().await?;
        }

        // Interactive mode
        if self.args.output_format == OutputFormat::Text {
            self.display.terminal().print_welcome()?;
            self.print_session_info()?;
        }

        // Wait for connection ready
        if let Some(BackendEvent::Connected) = self.client.recv().await {
            // Connection established
        }

        // Main REPL loop
        while self.running {
            // Reset interrupt flag
            self.interrupted.store(false, Ordering::SeqCst);

            // Read input
            let input = match self.read_input() {
                Ok(input) => input,
                Err(ReadlineError::Interrupted) => {
                    // Ctrl+C - cancel current input
                    println!();
                    continue;
                }
                Err(ReadlineError::Eof) => {
                    // Ctrl+D - exit
                    break;
                }
                Err(e) => {
                    self.display.terminal().print_error(&e.to_string())?;
                    continue;
                }
            };

            let input = input.trim();
            if input.is_empty() {
                continue;
            }

            // Handle built-in commands
            if input.starts_with('/') {
                if self.handle_command(input).await? {
                    continue;
                }
            }

            // Send to backend
            self.send_and_receive(input).await?;

            // Update session
            if let Some(ref mut session) = self.current_session {
                session.update_last_message(input);
                self.session_store.save(session).await?;
            }
        }

        // Cleanup
        self.client.close().await?;

        Ok(())
    }

    /// Show session picker
    async fn show_session_picker(&mut self) -> Result<()> {
        let sessions = self.session_store.list(SessionFilter::new().with_limit(20)).await?;

        if sessions.is_empty() {
            self.display.terminal().print_info("No previous sessions found. Starting new session.")?;
            self.create_new_session().await?;
            return Ok(());
        }

        // Check if we're in a TTY
        if atty::is(atty::Stream::Stdout) {
            let mut picker = SessionPicker::new(sessions);
            if let Some(session) = picker.show()? {
                self.current_session = Some(session.clone());
                self.client.set_project_id(Some(session.backend_session_id.clone()));
                self.display.terminal().print_success(&format!(
                    "Resumed session: {}",
                    session.display_name()
                ))?;
            } else {
                self.display.terminal().print_info("No session selected. Starting new session.")?;
                self.create_new_session().await?;
            }
        } else {
            // Non-TTY: just list sessions
            simple_session_list(&sessions);
            self.create_new_session().await?;
        }

        Ok(())
    }

    /// Continue the most recent session
    async fn continue_recent_session(&mut self) -> Result<()> {
        // Try to find session for current project first
        let session = if let Some(ref project) = self.project {
            self.session_store
                .get_most_recent_for_project(&project.root)
                .await?
        } else {
            None
        };

        // Fall back to most recent overall
        let session = match session {
            Some(s) => Some(s),
            None => self.session_store.get_most_recent().await?,
        };

        match session {
            Some(session) => {
                self.current_session = Some(session.clone());
                self.client.set_project_id(Some(session.backend_session_id.clone()));
                if self.args.output_format == OutputFormat::Text {
                    self.display.terminal().print_success(&format!(
                        "Continuing session: {} ({} messages)",
                        session.display_name(),
                        session.message_count
                    ))?;
                }
            }
            None => {
                if self.args.output_format == OutputFormat::Text {
                    self.display.terminal().print_info("No previous sessions found. Starting new session.")?;
                }
                self.create_new_session().await?;
            }
        }

        Ok(())
    }

    /// Resume a specific session by ID
    async fn resume_session(&mut self, session_id: &str) -> Result<()> {
        // Try exact match first
        let session = self.session_store.get(session_id).await?;

        // Try prefix match if exact match fails
        let session = match session {
            Some(s) => Some(s),
            None => {
                let sessions = self.session_store.list(SessionFilter::new()).await?;
                sessions.into_iter().find(|s| s.id.starts_with(session_id))
            }
        };

        match session {
            Some(session) => {
                self.current_session = Some(session.clone());
                self.client.set_project_id(Some(session.backend_session_id.clone()));
                if self.args.output_format == OutputFormat::Text {
                    self.display.terminal().print_success(&format!(
                        "Resumed session: {}",
                        session.display_name()
                    ))?;
                }
            }
            None => {
                self.display.terminal().print_error(&format!(
                    "Session not found: {}",
                    session_id
                ))?;
                self.create_new_session().await?;
            }
        }

        Ok(())
    }

    /// Create a new session
    async fn create_new_session(&mut self) -> Result<()> {
        // Generate backend session ID
        let backend_session_id = format!("cli-{}", uuid::Uuid::new_v4());

        let project_path = self.project.as_ref().map(|p| p.root.clone());
        let session = CliSession::new(backend_session_id.clone(), project_path);

        self.session_store.save(&session).await?;
        self.current_session = Some(session);
        self.client.set_project_id(Some(backend_session_id));

        Ok(())
    }

    /// Print session info
    fn print_session_info(&self) -> Result<()> {
        if let Some(ref project) = self.project {
            let header = crate::cli::project::build_context_header(project);
            self.display.terminal().print_info(&header)?;
        }
        Ok(())
    }

    /// Run in one-shot mode
    async fn run_one_shot(&mut self) -> Result<()> {
        let prompt = self.args.get_prompt()
            .context("No prompt provided for one-shot mode")?;

        // Handle session flags in one-shot mode too
        if self.args.continue_session {
            self.continue_recent_session().await?;
        } else if let Some(session_id) = self.args.get_resume_session_id().map(|s| s.to_string()) {
            self.resume_session(&session_id).await?;
        } else {
            self.create_new_session().await?;
        }

        // Wait for connection
        if let Some(BackendEvent::Connected) = self.client.recv().await {
            // Connection established
        }

        // Send prompt and receive response
        self.send_and_receive(&prompt).await?;

        // Update session
        if let Some(ref mut session) = self.current_session {
            session.update_last_message(&prompt);
            self.session_store.save(session).await?;
        }

        // Close connection
        self.client.close().await?;

        Ok(())
    }

    /// Read input from user
    fn read_input(&mut self) -> Result<String, ReadlineError> {
        if self.args.output_format == OutputFormat::Text {
            self.display.terminal().print_prompt().ok();
        }
        self.editor.readline("")
    }

    /// Handle a built-in command
    async fn handle_command(&mut self, input: &str) -> Result<bool> {
        let parts: Vec<&str> = input.splitn(2, ' ').collect();
        let command = parts[0];
        let cmd_args = parts.get(1).copied();

        match command {
            "/help" | "/h" | "/?" => {
                self.display.terminal().print_help()?;
                Ok(true)
            }
            "/quit" | "/q" | "/exit" => {
                self.running = false;
                Ok(true)
            }
            "/clear" | "/cls" => {
                print!("\x1B[2J\x1B[1;1H");
                Ok(true)
            }
            "/sessions" => {
                self.list_sessions().await?;
                Ok(true)
            }
            "/session" => {
                self.print_current_session()?;
                Ok(true)
            }
            "/commands" => {
                // Send to backend to list commands
                self.client.send_command("commands", None).await?;
                self.receive_response().await?;
                Ok(true)
            }
            "/checkpoints" => {
                self.client.send_command("checkpoints", None).await?;
                self.receive_response().await?;
                Ok(true)
            }
            "/rewind" => {
                if let Some(id) = cmd_args {
                    let args = serde_json::json!({ "checkpoint_id": id });
                    self.client.send_command("rewind", Some(args)).await?;
                    self.receive_response().await?;
                } else {
                    self.display.terminal().print_error("Usage: /rewind <checkpoint_id>")?;
                }
                Ok(true)
            }
            _ => {
                // Not a built-in command, might be a custom command
                // Send to backend as chat (it will handle slash commands)
                Ok(false)
            }
        }
    }

    /// List recent sessions
    async fn list_sessions(&mut self) -> Result<()> {
        let sessions = self.session_store.list(SessionFilter::new().with_limit(10)).await?;
        simple_session_list(&sessions);
        Ok(())
    }

    /// Print current session info
    fn print_current_session(&self) -> Result<()> {
        if let Some(ref session) = self.current_session {
            println!("Session ID: {}", session.id);
            println!("Name: {}", session.display_name());
            println!("Messages: {}", session.message_count);
            println!("Last active: {}", session.last_active_display());
            if let Some(ref path) = session.project_path {
                println!("Project: {}", path.display());
            }
        } else {
            println!("No active session");
        }
        Ok(())
    }

    /// Send a message and receive the response
    async fn send_and_receive(&mut self, content: &str) -> Result<()> {
        // Build metadata from project context
        let metadata = self.project.as_ref().map(build_metadata);

        // Send chat message
        self.client.send_chat(content, metadata).await?;

        // Receive response
        self.receive_response().await
    }

    /// Receive and display response from backend
    async fn receive_response(&mut self) -> Result<()> {
        let mut completed = false;

        while !completed {
            // Check for interrupt
            if self.interrupted.load(Ordering::SeqCst) {
                self.display.terminal_mut().stop_spinner();
                self.display.terminal().print_warning("Interrupted")?;
                break;
            }

            // Receive with timeout
            tokio::select! {
                event = self.client.recv() => {
                    match event {
                        Some(event) => {
                            self.display.handle_event(&event)?;

                            // Check for completion
                            match &event {
                                BackendEvent::ChatComplete { .. } => completed = true,
                                BackendEvent::OperationEvent(op) => {
                                    match op {
                                        crate::cli::ws_client::OperationEvent::Completed { .. } |
                                        crate::cli::ws_client::OperationEvent::Failed { .. } => {
                                            completed = true;
                                        }
                                        _ => {}
                                    }
                                }
                                BackendEvent::Error { .. } => completed = true,
                                BackendEvent::Disconnected => {
                                    completed = true;
                                    self.running = false;
                                }
                                _ => {}
                            }
                        }
                        None => {
                            // Channel closed
                            completed = true;
                            self.running = false;
                        }
                    }
                }
                _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
                    // Check for interrupt periodically
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_parsing() {
        let input = "/rewind checkpoint-123";
        let parts: Vec<&str> = input.splitn(2, ' ').collect();
        assert_eq!(parts[0], "/rewind");
        assert_eq!(parts.get(1), Some(&"checkpoint-123"));
    }
}
