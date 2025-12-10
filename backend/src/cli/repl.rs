// backend/src/cli/repl.rs
// Interactive REPL loop for Mira CLI

use anyhow::{Context, Result};
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::cli::args::{CliArgs, OutputFormat};
use crate::cli::commands::{AgentAction, BuiltinCommand, CommandLoader, ReviewTarget};
use crate::cli::config::CliConfig;
use crate::cli::display::{StreamingDisplay, TerminalDisplay};
use crate::cli::project::{build_metadata, ProjectDetector, DetectedProject};
use crate::cli::session::{simple_session_list, CliSession, SessionPicker};
use crate::cli::ws_client::{BackendEvent, MiraClient};

/// REPL state
pub struct Repl {
    /// CLI arguments
    args: CliArgs,
    /// CLI configuration
    #[allow(dead_code)]
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
    /// Current session
    current_session: Option<CliSession>,
    /// Detected project context
    project: Option<DetectedProject>,
    /// Custom command loader
    command_loader: CommandLoader,
}

impl Repl {
    /// Create a new REPL instance
    pub async fn new(args: CliArgs) -> Result<Self> {
        // Load config
        let config = CliConfig::load().unwrap_or_default();

        // Ensure directories exist
        CliConfig::ensure_dirs()?;

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

        // Load custom commands
        let command_loader = CommandLoader::new()
            .unwrap_or_default();

        Ok(Self {
            args,
            config,
            client,
            display,
            editor,
            interrupted,
            running: true,
            current_session: None,
            project,
            command_loader,
        })
    }

    /// Run the REPL
    pub async fn run(&mut self) -> Result<()> {
        // Check for one-shot mode
        if self.args.is_non_interactive() {
            return self.run_one_shot().await;
        }

        // Handle session flags
        if let Some(fork_id) = self.args.fork.clone() {
            // Fork from existing session
            self.fork_session(&fork_id).await?;
        } else if self.args.should_show_picker() {
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

        // Note: connection_ready is consumed during session operations,
        // so we don't need to wait for it here. Session success implies connection.

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

            // Update local session tracking (backend handles persistence via update_session_on_message)
            if let Some(ref mut session) = self.current_session {
                session.update_last_message(input);
            }
        }

        // Cleanup
        self.client.close().await?;

        Ok(())
    }

    /// Show session picker
    async fn show_session_picker(&mut self) -> Result<()> {
        // Get project path for filtering
        let project_path = self.project.as_ref().map(|p| p.root.to_string_lossy().to_string());

        // List sessions from backend
        let backend_sessions = self.client.list_sessions(
            project_path.as_deref(),
            None,
            Some(20),
        ).await?;

        // Convert to CLI sessions
        let sessions: Vec<CliSession> = backend_sessions.into_iter()
            .map(CliSession::from_backend)
            .collect();

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
                self.client.set_project_id(Some(session.id.clone()));
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
        // Get project path for filtering
        let project_path = self.project.as_ref().map(|p| p.root.to_string_lossy().to_string());

        // Try to find session for current project first, then fall back to any session
        let backend_sessions = self.client.list_sessions(
            project_path.as_deref(),
            None,
            Some(1),
        ).await?;

        // If no project-specific session, try listing all
        let backend_sessions = if backend_sessions.is_empty() && project_path.is_some() {
            self.client.list_sessions(None, None, Some(1)).await?
        } else {
            backend_sessions
        };

        match backend_sessions.into_iter().next() {
            Some(backend_session) => {
                let session = CliSession::from_backend(backend_session);
                self.current_session = Some(session.clone());
                self.client.set_project_id(Some(session.id.clone()));
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
        let backend_session = self.client.get_session(session_id).await;

        // If exact match fails, try prefix match
        let backend_session = match backend_session {
            Ok(s) => Some(s),
            Err(_) => {
                // List sessions and try prefix match
                let sessions = self.client.list_sessions(None, None, Some(50)).await?;
                sessions.into_iter().find(|s| s.id.starts_with(session_id))
            }
        };

        match backend_session {
            Some(bs) => {
                let session = CliSession::from_backend(bs);
                self.current_session = Some(session.clone());
                self.client.set_project_id(Some(session.id.clone()));
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
        // Get project path for the session
        let project_path = self.project.as_ref().map(|p| p.root.to_string_lossy().to_string());

        // Create session in backend
        let backend_session = self.client.create_session(
            None, // name - will auto-generate
            project_path.as_deref(),
        ).await?;

        let session = CliSession::from_backend(backend_session);
        let session_id = session.id.clone();

        self.current_session = Some(session);
        self.client.set_project_id(Some(session_id));

        Ok(())
    }

    /// Fork from an existing session
    async fn fork_session(&mut self, source_session_id: &str) -> Result<()> {
        // Try to fork via backend API
        let fork_result = self.client.fork_session(source_session_id, None).await;

        // If exact match fails, try prefix match
        let forked_session = match fork_result {
            Ok(s) => Some(s),
            Err(_) => {
                // List sessions and try prefix match
                let sessions = self.client.list_sessions(None, None, Some(50)).await?;
                if let Some(source) = sessions.into_iter().find(|s| s.id.starts_with(source_session_id)) {
                    // Fork from the matched session
                    self.client.fork_session(&source.id, None).await.ok()
                } else {
                    None
                }
            }
        };

        match forked_session {
            Some(backend_session) => {
                let session = CliSession::from_backend(backend_session);
                let session_id = session.id.clone();

                self.current_session = Some(session.clone());
                self.client.set_project_id(Some(session_id));

                if self.args.output_format == OutputFormat::Text {
                    self.display.terminal().print_success(&format!(
                        "Forked session: {}",
                        session.display_name()
                    ))?;
                }
            }
            None => {
                self.display.terminal().print_error(&format!(
                    "Session not found: {}",
                    source_session_id
                ))?;
                self.create_new_session().await?;
            }
        }

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
        if let Some(fork_id) = self.args.fork.clone() {
            self.fork_session(&fork_id).await?;
        } else if self.args.continue_session {
            self.continue_recent_session().await?;
        } else if let Some(session_id) = self.args.get_resume_session_id().map(|s| s.to_string()) {
            self.resume_session(&session_id).await?;
        } else {
            self.create_new_session().await?;
        }

        // Note: connection_ready is consumed during session operations,
        // so we don't need to wait for it here. Session success implies connection.

        // Expand custom commands if the prompt starts with /
        let final_prompt = if prompt.starts_with('/') {
            let parts: Vec<&str> = prompt.splitn(2, ' ').collect();
            let cmd_name = parts[0].trim_start_matches('/');
            let cmd_args = parts.get(1).copied();

            // Try to expand as custom command
            self.command_loader.expand(cmd_name, cmd_args).unwrap_or(prompt.clone())
        } else {
            prompt.clone()
        };

        // Send prompt and receive response
        self.send_and_receive(&final_prompt).await?;

        // Update local session tracking (backend handles persistence via update_session_on_message)
        if let Some(ref mut session) = self.current_session {
            session.update_last_message(&prompt);
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

        // First, try to parse as a new builtin command
        if let Some(builtin) = BuiltinCommand::parse(input) {
            return self.handle_builtin_command(builtin).await;
        }

        match command {
            "/help" | "/h" | "/?" => {
                self.print_full_help()?;
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
                // List custom commands
                self.list_custom_commands()?;
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
                // Check for custom command
                let cmd_name = command.trim_start_matches('/');
                if let Some(expanded) = self.command_loader.expand(cmd_name, cmd_args) {
                    // Custom command found - send expanded template as chat
                    self.send_and_receive(&expanded).await?;
                    Ok(true)
                } else {
                    // Not a built-in or custom command
                    // Send to backend as chat (it might handle it)
                    Ok(false)
                }
            }
        }
    }

    /// Handle new builtin commands
    async fn handle_builtin_command(&mut self, cmd: BuiltinCommand) -> Result<bool> {
        match cmd {
            BuiltinCommand::Resume { target, last } => {
                self.handle_resume_command(target, last).await?;
                Ok(true)
            }
            BuiltinCommand::Review { target } => {
                self.handle_review_command(target).await?;
                Ok(true)
            }
            BuiltinCommand::Rename { name } => {
                self.handle_rename_command(&name).await?;
                Ok(true)
            }
            BuiltinCommand::Agents { action } => {
                self.handle_agents_command(action).await?;
                Ok(true)
            }
            BuiltinCommand::Search { query, search_type, num_results } => {
                self.handle_search_command(&query, search_type.as_deref(), num_results).await?;
                Ok(true)
            }
            BuiltinCommand::Status => {
                self.print_current_session()?;
                Ok(true)
            }
        }
    }

    /// Handle /resume command
    async fn handle_resume_command(&mut self, target: Option<String>, last: bool) -> Result<()> {
        if last {
            // Resume most recent session
            self.continue_recent_session().await?;
            self.print_session_info()?;
            return Ok(());
        }

        match target {
            Some(name_or_id) => {
                // Try to find session by name first, then by ID
                let backend_sessions = self.client.list_sessions(None, Some(&name_or_id), Some(50)).await?;

                // Check for exact name match
                let session = backend_sessions.iter()
                    .find(|s| s.name.as_ref().map(|n| n == &name_or_id).unwrap_or(false))
                    .or_else(|| backend_sessions.iter().find(|s| s.id.starts_with(&name_or_id)));

                if let Some(bs) = session {
                    let session = CliSession::from_backend(bs.clone());
                    self.current_session = Some(session.clone());
                    self.client.set_project_id(Some(session.id.clone()));
                    self.display.terminal().print_success(&format!(
                        "Resumed session: {}",
                        session.display_name()
                    ))?;
                    self.print_session_info()?;
                } else {
                    self.display.terminal().print_error(&format!(
                        "Session not found: {}",
                        name_or_id
                    ))?;
                }
            }
            None => {
                // Show session picker
                self.show_session_picker().await?;
                self.print_session_info()?;
            }
        }
        Ok(())
    }

    /// Handle /review command
    async fn handle_review_command(&mut self, target: ReviewTarget) -> Result<()> {
        // Get the diff based on target
        let (diff, description) = match &target {
            ReviewTarget::Uncommitted => {
                let output = std::process::Command::new("git")
                    .args(["diff", "HEAD"])
                    .current_dir(self.project.as_ref().map(|p| &p.root).unwrap_or(&std::env::current_dir()?))
                    .output()
                    .context("Failed to run git diff")?;
                (
                    String::from_utf8_lossy(&output.stdout).to_string(),
                    "uncommitted changes".to_string()
                )
            }
            ReviewTarget::Branch { base } => {
                let output = std::process::Command::new("git")
                    .args(["diff", &format!("{}...HEAD", base)])
                    .current_dir(self.project.as_ref().map(|p| &p.root).unwrap_or(&std::env::current_dir()?))
                    .output()
                    .context("Failed to run git diff")?;
                (
                    String::from_utf8_lossy(&output.stdout).to_string(),
                    format!("changes against {}", base)
                )
            }
            ReviewTarget::Commit { hash } => {
                let output = std::process::Command::new("git")
                    .args(["show", hash, "--format="])
                    .current_dir(self.project.as_ref().map(|p| &p.root).unwrap_or(&std::env::current_dir()?))
                    .output()
                    .context("Failed to run git show")?;
                (
                    String::from_utf8_lossy(&output.stdout).to_string(),
                    format!("commit {}", hash)
                )
            }
            ReviewTarget::Staged => {
                let output = std::process::Command::new("git")
                    .args(["diff", "--cached"])
                    .current_dir(self.project.as_ref().map(|p| &p.root).unwrap_or(&std::env::current_dir()?))
                    .output()
                    .context("Failed to run git diff --cached")?;
                (
                    String::from_utf8_lossy(&output.stdout).to_string(),
                    "staged changes".to_string()
                )
            }
        };

        if diff.trim().is_empty() {
            self.display.terminal().print_info(&format!("No {} to review", description))?;
            return Ok(());
        }

        // Show diff stats
        let lines: Vec<&str> = diff.lines().collect();
        let additions = lines.iter().filter(|l| l.starts_with('+') && !l.starts_with("+++")).count();
        let deletions = lines.iter().filter(|l| l.starts_with('-') && !l.starts_with("---")).count();

        self.display.terminal().print_info(&format!(
            "Reviewing {}: +{} -{} lines",
            description, additions, deletions
        ))?;

        // Send to LLM for review
        let prompt = format!(
            "Please review the following code changes ({}).\n\n\
            Provide:\n\
            1. A brief summary of what changed\n\
            2. Any potential issues or bugs\n\
            3. Suggestions for improvement\n\
            4. Security considerations if applicable\n\n\
            ```diff\n{}\n```",
            description, diff
        );

        self.send_and_receive(&prompt).await?;
        Ok(())
    }

    /// Handle /rename command
    async fn handle_rename_command(&mut self, name: &str) -> Result<()> {
        if let Some(ref session) = self.current_session {
            // Send rename request to backend
            let args = serde_json::json!({
                "session_id": session.id,
                "name": name
            });
            self.client.send_command("session.update", Some(args)).await?;

            // Update local session
            if let Some(ref mut s) = self.current_session {
                s.name = Some(name.to_string());
            }

            self.display.terminal().print_success(&format!("Session renamed to: {}", name))?;
        } else {
            self.display.terminal().print_error("No active session to rename")?;
        }
        Ok(())
    }

    /// Handle /agents command
    async fn handle_agents_command(&mut self, action: AgentAction) -> Result<()> {
        match action {
            AgentAction::List => {
                // Get active Codex sessions from backend
                if let Some(ref session) = self.current_session {
                    let args = serde_json::json!({
                        "voice_session_id": session.id
                    });
                    self.client.send_command("session.active_agents", Some(args)).await?;
                    self.receive_response().await?;
                } else {
                    self.display.terminal().print_info("No active session")?;
                }
            }
            AgentAction::Cancel { agent_id } => {
                if agent_id.is_empty() {
                    self.display.terminal().print_error("Usage: /agents cancel <agent_id>")?;
                    return Ok(());
                }
                let args = serde_json::json!({
                    "codex_session_id": agent_id
                });
                self.client.send_command("session.cancel_agent", Some(args)).await?;
                self.receive_response().await?;
            }
            AgentAction::Show { agent_id } => {
                if agent_id.is_empty() {
                    self.display.terminal().print_error("Usage: /agents show <agent_id>")?;
                    return Ok(());
                }
                let args = serde_json::json!({
                    "codex_session_id": agent_id
                });
                self.client.send_command("session.agent_info", Some(args)).await?;
                self.receive_response().await?;
            }
        }
        Ok(())
    }

    /// Handle /search command
    async fn handle_search_command(
        &mut self,
        query: &str,
        search_type: Option<&str>,
        num_results: Option<usize>,
    ) -> Result<()> {
        self.display.terminal().print_info(&format!("Searching: {}", query))?;

        // Build prompt that uses the web_search tool
        let type_hint = match search_type {
            Some("docs") | Some("documentation") => " in documentation",
            Some("github") => " on GitHub",
            Some("stackoverflow") | Some("so") => " on Stack Overflow",
            _ => "",
        };

        let num = num_results.unwrap_or(5);

        let prompt = format!(
            "Search the web for: \"{}\"{}\n\n\
            Please provide {} search results with titles, URLs, and brief descriptions.",
            query, type_hint, num
        );

        self.send_and_receive(&prompt).await?;
        Ok(())
    }

    /// Print full help including builtin commands
    fn print_full_help(&self) -> Result<()> {
        self.display.terminal().print_help()?;
        println!("{}", BuiltinCommand::help());
        Ok(())
    }

    /// List available custom commands
    fn list_custom_commands(&self) -> Result<()> {
        let commands = self.command_loader.list();

        println!("\n  Built-in commands:");
        println!("    /help, /h, /?     - Show help");
        println!("    /quit, /q, /exit  - Exit the REPL");
        println!("    /clear, /cls      - Clear the screen");
        println!("    /sessions         - List recent sessions");
        println!("    /session          - Show current session info");
        println!("    /commands         - List available commands");
        println!("    /checkpoints      - List conversation checkpoints");
        println!("    /rewind <id>      - Rewind to a checkpoint");

        if !commands.is_empty() {
            println!("\n  Custom commands:");
            for cmd in commands {
                let args_hint = if cmd.accepts_args { " <args>" } else { "" };
                println!("    /{}{} - {}", cmd.name, args_hint, cmd.description);
            }
        }

        println!();
        Ok(())
    }

    /// List recent sessions
    async fn list_sessions(&mut self) -> Result<()> {
        let backend_sessions = self.client.list_sessions(None, None, Some(10)).await?;
        let sessions: Vec<CliSession> = backend_sessions.into_iter()
            .map(CliSession::from_backend)
            .collect();
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
                            // Check for sudo approval request BEFORE display handling
                            if let BackendEvent::OperationEvent(
                                crate::cli::ws_client::OperationEvent::SudoApprovalRequired {
                                    approval_request_id,
                                    command,
                                    reason,
                                    ..
                                }
                            ) = &event {
                                // Handle sudo approval interactively
                                self.handle_sudo_approval(
                                    approval_request_id,
                                    command,
                                    reason.as_deref(),
                                ).await?;
                                // Continue waiting for operation completion
                                continue;
                            }

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

    /// Handle sudo approval request interactively
    async fn handle_sudo_approval(
        &mut self,
        approval_request_id: &str,
        command: &str,
        reason: Option<&str>,
    ) -> Result<()> {
        use std::io::{self, Write};

        // Stop any spinner
        self.display.terminal_mut().stop_spinner();

        // Display the approval request
        println!();
        self.display.terminal().print_warning("Privileged command requires approval")?;
        println!();
        println!("  Command: \x1b[1;33m{}\x1b[0m", command);
        if let Some(r) = reason {
            println!("  Reason:  {}", r);
        }
        println!();

        // Check if we can prompt interactively
        if !atty::is(atty::Stream::Stdin) {
            // Non-interactive mode - auto-deny for safety
            self.display.terminal().print_error("Auto-denied (non-interactive mode)")?;
            self.client.deny_sudo_request(
                approval_request_id,
                Some("Auto-denied: non-interactive mode cannot prompt for approval"),
            ).await?;
            println!();
            return Ok(());
        }

        // Prompt for approval
        print!("  Approve this command? [Y/n]: ");
        io::stdout().flush()?;

        // Read user input
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim().to_lowercase();

        // Process response
        if input.is_empty() || input == "y" || input == "yes" {
            self.display.terminal().print_success("Approved")?;
            self.client.approve_sudo_request(approval_request_id).await?;
        } else {
            self.display.terminal().print_error("Denied")?;
            self.client.deny_sudo_request(approval_request_id, Some("User denied from CLI")).await?;
        }

        println!();
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
