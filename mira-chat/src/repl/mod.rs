//! Interactive REPL for Mira Chat
//!
//! Provides a readline-based interface with:
//! - Command history
//! - Multi-line input support
//! - Streaming response display
//! - Tool execution feedback

pub mod colors;
mod commands;
mod execution;
mod formatter;
mod helper;
pub mod spend;
mod streaming;

use anyhow::Result;
use rustyline::error::ReadlineError;
use rustyline::history::DefaultHistory;
use rustyline::Editor;
use sqlx::SqlitePool;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use crate::context::MiraContext;
use crate::responses::Client;
use crate::semantic::SemanticSearch;
use crate::session::SessionManager;
use crate::tools::ToolExecutor;

use commands::CommandHandler;
use execution::{execute, ExecutionConfig};
use helper::MiraHelper;
use spend::SpendTracker;

/// REPL state
pub struct Repl {
    /// Readline editor with history and completion
    editor: Editor<MiraHelper, DefaultHistory>,
    /// GPT-5.2 API client
    client: Option<Client>,
    /// Tool executor
    tools: ToolExecutor,
    /// Mira context (fallback if no session)
    context: MiraContext,
    /// Previous response ID for conversation continuity (fallback if no session)
    previous_response_id: Option<String>,
    /// History file path
    history_path: std::path::PathBuf,
    /// Database pool for slash commands
    db: Option<SqlitePool>,
    /// Semantic search for slash commands
    semantic: Arc<SemanticSearch>,
    /// Session manager for invisible persistence
    session: Option<Arc<SessionManager>>,
    /// Cancellation flag for Ctrl+C during streaming
    cancelled: Arc<AtomicBool>,
    /// When this REPL instance started (used for /uptime)
    start_time: Instant,
    /// Session spend tracker
    spend: SpendTracker,
}

impl Repl {
    pub fn new(
        db: Option<SqlitePool>,
        semantic: Arc<SemanticSearch>,
        session: Option<Arc<SessionManager>>,
    ) -> Result<Self> {
        let mut editor = Editor::new()?;
        editor.set_helper(Some(MiraHelper::new()));

        // History file in ~/.mira/chat_history
        let history_path = dirs::home_dir()
            .unwrap_or_default()
            .join(".mira")
            .join("chat_history");

        // Build ToolExecutor with db, semantic, and session if available
        let mut tools = ToolExecutor::new().with_semantic(Arc::clone(&semantic));
        if let Some(ref pool) = db {
            tools = tools.with_db(pool.clone());
        }
        if let Some(ref sess) = session {
            tools = tools.with_session(Arc::clone(sess));
        }

        Ok(Self {
            editor,
            client: None,
            tools,
            context: MiraContext::default(),
            previous_response_id: None,
            history_path,
            db,
            semantic,
            session,
            cancelled: Arc::new(AtomicBool::new(false)),
            start_time: Instant::now(),
            spend: SpendTracker::new(),
        })
    }

    /// Initialize the client with API key
    pub fn with_api_key(mut self, api_key: String) -> Self {
        self.client = Some(Client::new(api_key));
        self
    }

    /// Set pre-loaded context
    pub fn with_loaded_context(mut self, context: MiraContext) -> Self {
        self.context = context;
        self
    }

    /// Load command history
    fn load_history(&mut self) {
        if self.history_path.exists() {
            let _ = self.editor.load_history(&self.history_path);
        }
    }

    /// Save command history
    fn save_history(&mut self) {
        if let Some(parent) = self.history_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = self.editor.save_history(&self.history_path);
    }

    /// Run the REPL loop
    pub async fn run(&mut self) -> Result<()> {
        self.load_history();

        // Set up Ctrl+C handler for cancelling in-flight requests
        let cancelled = Arc::clone(&self.cancelled);
        tokio::spawn(async move {
            loop {
                if tokio::signal::ctrl_c().await.is_ok() {
                    cancelled.store(true, Ordering::SeqCst);
                }
            }
        });

        println!("{}", colors::status("Type your message (Ctrl+D to exit, /help for commands)"));
        println!("{}", colors::status("  Use \\\\ at end of line for multi-line input, or \"\"\" to start/end block"));
        println!("{}", colors::status("  Press Ctrl+C to cancel in-flight requests"));
        println!();

        loop {
            let input = self.read_input()?;

            match input {
                Some(line) => {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }

                    self.editor.add_history_entry(&line)?;

                    // Handle slash commands
                    if trimmed.starts_with('/') {
                        self.handle_command(trimmed).await?;
                        continue;
                    }

                    // Reset cancellation flag before processing
                    self.cancelled.store(false, Ordering::SeqCst);

                    // Process user input with streaming
                    self.process_input(trimmed).await?;
                }
                None => {
                    println!("Goodbye!");
                    break;
                }
            }
        }

        self.save_history();
        Ok(())
    }

    /// Handle slash commands using CommandHandler
    async fn handle_command(&mut self, cmd: &str) -> Result<()> {
        let mut handler = CommandHandler {
            context: &mut self.context,
            previous_response_id: &mut self.previous_response_id,
            db: &self.db,
            semantic: &self.semantic,
            session: &self.session,
            client: &self.client,
            start_time: self.start_time,
        };
        handler.handle(cmd).await?;
        Ok(())
    }

    /// Process user input using execution module
    async fn process_input(&mut self, input: &str) -> Result<()> {
        let client = match &self.client {
            Some(c) => c,
            None => {
                eprintln!("Error: API client not initialized");
                return Ok(());
            }
        };

        let config = ExecutionConfig {
            client,
            tools: &self.tools,
            context: &self.context,
            session: &self.session,
            cancelled: &self.cancelled,
        };

        let result = execute(input, config).await?;
        self.previous_response_id = result.response_id;

        // Track session spend
        self.spend.add_usage(&result.usage);
        println!("  {}", colors::status(&format!("[session: {}]", self.spend.format_spend())));

        // Check for spend warnings
        if let Some(warning) = self.spend.check_warnings() {
            println!("  {}", colors::warning(&warning));
        }

        Ok(())
    }

    /// Read input with multi-line support
    fn read_input(&mut self) -> Result<Option<String>> {
        let first_line = match self.editor.readline(&colors::prompt()) {
            Ok(line) => line,
            Err(ReadlineError::Interrupted) => {
                println!("^C");
                return Ok(Some(String::new())); // Empty to continue loop
            }
            Err(ReadlineError::Eof) => return Ok(None),
            Err(err) => {
                eprintln!("Error: {:?}", err);
                return Ok(None);
            }
        };

        let trimmed = first_line.trim();

        // Check for triple-quote multi-line mode
        if trimmed == "\"\"\"" || trimmed.starts_with("\"\"\"") {
            return self.read_multiline_block(&first_line);
        }

        // Check for backslash continuation
        if trimmed.ends_with('\\') {
            return self.read_continuation_lines(&first_line);
        }

        Ok(Some(first_line))
    }

    /// Read multi-line block delimited by """
    fn read_multiline_block(&mut self, first_line: &str) -> Result<Option<String>> {
        let mut lines = Vec::new();

        // Handle content after opening """
        let after_open = first_line.trim().strip_prefix("\"\"\"").unwrap_or("");
        if !after_open.is_empty() {
            if after_open.ends_with("\"\"\"") {
                // Single line with """ on both ends
                return Ok(Some(
                    after_open
                        .strip_suffix("\"\"\"")
                        .unwrap_or(after_open)
                        .to_string(),
                ));
            }
            lines.push(after_open.to_string());
        }

        loop {
            match self.editor.readline(&colors::continuation_prompt()) {
                Ok(line) => {
                    if line.trim() == "\"\"\"" || line.trim().ends_with("\"\"\"") {
                        // End of block
                        let before_close = line.trim().strip_suffix("\"\"\"").unwrap_or("");
                        if !before_close.is_empty() {
                            lines.push(before_close.to_string());
                        }
                        break;
                    }
                    lines.push(line);
                }
                Err(ReadlineError::Interrupted) => {
                    println!("^C (cancelled multi-line)");
                    return Ok(Some(String::new()));
                }
                Err(ReadlineError::Eof) => {
                    return Ok(None);
                }
                Err(err) => {
                    eprintln!("Error: {:?}", err);
                    return Ok(None);
                }
            }
        }

        Ok(Some(lines.join("\n")))
    }

    /// Read continuation lines (ending with \)
    fn read_continuation_lines(&mut self, first_line: &str) -> Result<Option<String>> {
        let mut lines = Vec::new();
        lines.push(
            first_line
                .trim()
                .strip_suffix('\\')
                .unwrap_or(first_line.trim())
                .to_string(),
        );

        loop {
            match self.editor.readline(&colors::continuation_prompt()) {
                Ok(line) => {
                    let trimmed = line.trim();
                    if trimmed.ends_with('\\') {
                        lines.push(trimmed.strip_suffix('\\').unwrap_or(trimmed).to_string());
                    } else {
                        lines.push(line);
                        break;
                    }
                }
                Err(ReadlineError::Interrupted) => {
                    println!("^C (cancelled multi-line)");
                    return Ok(Some(String::new()));
                }
                Err(ReadlineError::Eof) => {
                    return Ok(None);
                }
                Err(err) => {
                    eprintln!("Error: {:?}", err);
                    return Ok(None);
                }
            }
        }

        Ok(Some(lines.join("\n")))
    }
}

/// Entry point for the REPL with pre-loaded context
pub async fn run_with_context(
    api_key: String,
    context: MiraContext,
    db: Option<SqlitePool>,
    semantic: Arc<SemanticSearch>,
    session: Option<Arc<SessionManager>>,
) -> Result<()> {
    let mut repl = Repl::new(db, semantic, session)?
        .with_api_key(api_key)
        .with_loaded_context(context);

    repl.run().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_repl_new() {
        // Create with no db, semantic, or session
        let semantic = Arc::new(SemanticSearch::new(None, None).await);
        let repl = Repl::new(None, semantic, None);
        assert!(repl.is_ok());
    }
}
