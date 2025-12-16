//! Interactive REPL for Mira Chat
//!
//! Provides a readline-based interface with:
//! - Command history
//! - Multi-line input support
//! - Streaming response display
//! - Tool execution feedback

use anyhow::Result;
use rustyline::completion::{Completer, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::{Hinter, HistoryHinter};
use rustyline::history::DefaultHistory;
use rustyline::validate::Validator;
use rustyline::{Context, Editor, Helper};
use sqlx::SqlitePool;
use std::borrow::Cow;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::context::{build_system_prompt, MiraContext};
use crate::reasoning::classify;
use crate::responses::{Client, ResponsesResponse, StreamEvent, Tool, Usage};
use crate::semantic::SemanticSearch;
use crate::tools::{get_tools, ToolExecutor};

/// Slash commands for tab completion
const SLASH_COMMANDS: &[&str] = &[
    "/help",
    "/clear",
    "/context",
    "/status",
    "/switch",
    "/remember",
    "/recall",
    "/tasks",
    "/quit",
    "/exit",
];

/// Custom helper for rustyline with completion and hints
struct MiraHelper {
    hinter: HistoryHinter,
}

impl MiraHelper {
    fn new() -> Self {
        Self {
            hinter: HistoryHinter::new(),
        }
    }
}

impl Completer for MiraHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        // Only complete slash commands at the start of the line
        if line.starts_with('/') && pos <= line.find(' ').unwrap_or(line.len()) {
            let matches: Vec<Pair> = SLASH_COMMANDS
                .iter()
                .filter(|cmd| cmd.starts_with(line.split_whitespace().next().unwrap_or("")))
                .map(|cmd| Pair {
                    display: cmd.to_string(),
                    replacement: cmd.to_string(),
                })
                .collect();
            Ok((0, matches))
        } else {
            Ok((pos, vec![]))
        }
    }
}

impl Hinter for MiraHelper {
    type Hint = String;

    fn hint(&self, line: &str, pos: usize, ctx: &Context<'_>) -> Option<String> {
        // Show history hints for non-slash commands
        if !line.starts_with('/') {
            self.hinter.hint(line, pos, ctx)
        } else {
            None
        }
    }
}

impl Highlighter for MiraHelper {
    fn highlight_hint<'h>(&self, hint: &'h str) -> Cow<'h, str> {
        // Dim the hint text
        Cow::Owned(format!("\x1b[90m{}\x1b[0m", hint))
    }
}

impl Validator for MiraHelper {}

impl Helper for MiraHelper {}

/// REPL state
pub struct Repl {
    /// Readline editor with history and completion
    editor: Editor<MiraHelper, DefaultHistory>,
    /// GPT-5.2 API client
    client: Option<Client>,
    /// Tool executor
    tools: ToolExecutor,
    /// Mira context
    context: MiraContext,
    /// Previous response ID for conversation continuity
    previous_response_id: Option<String>,
    /// History file path
    history_path: std::path::PathBuf,
    /// Database pool for slash commands
    db: Option<SqlitePool>,
    /// Semantic search for slash commands
    semantic: Arc<SemanticSearch>,
    /// Cancellation flag for Ctrl+C during streaming
    cancelled: Arc<AtomicBool>,
}

impl Repl {
    pub fn new(db: Option<SqlitePool>, semantic: Arc<SemanticSearch>) -> Result<Self> {
        let mut editor = Editor::new()?;
        editor.set_helper(Some(MiraHelper::new()));

        // History file in ~/.mira/chat_history
        let history_path = dirs::home_dir()
            .unwrap_or_default()
            .join(".mira")
            .join("chat_history");

        // Build ToolExecutor with db and semantic if available
        let mut tools = ToolExecutor::new().with_semantic(Arc::clone(&semantic));
        if let Some(ref pool) = db {
            tools = tools.with_db(pool.clone());
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
            cancelled: Arc::new(AtomicBool::new(false)),
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

        println!("Type your message (Ctrl+D to exit, /help for commands)");
        println!("  Use \\\\ at end of line for multi-line input, or \"\"\" to start/end block");
        println!("  Press Ctrl+C to cancel in-flight requests");
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
                    self.process_input_streaming(trimmed).await?;
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

    /// Read input with multi-line support
    fn read_input(&mut self) -> Result<Option<String>> {
        let first_line = match self.editor.readline(">>> ") {
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
                return Ok(Some(after_open.strip_suffix("\"\"\"").unwrap_or(after_open).to_string()));
            }
            lines.push(after_open.to_string());
        }

        loop {
            match self.editor.readline("... ") {
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
        lines.push(first_line.trim().strip_suffix('\\').unwrap_or(first_line.trim()).to_string());

        loop {
            match self.editor.readline("... ") {
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

    /// Handle slash commands
    async fn handle_command(&mut self, cmd: &str) -> Result<()> {
        let parts: Vec<&str> = cmd.splitn(2, ' ').collect();
        let command = parts[0];
        let arg = parts.get(1).copied().unwrap_or("");

        match command {
            "/help" => {
                println!("Commands:");
                println!("  /help              - Show this help");
                println!("  /clear             - Clear conversation history");
                println!("  /context           - Show current Mira context");
                println!("  /status            - Show current state");
                println!("  /switch [path]     - Switch project");
                println!("  /remember <text>   - Store in memory");
                println!("  /recall <query>    - Search memory");
                println!("  /tasks             - List tasks");
                println!("  /quit              - Exit");
            }
            "/clear" => {
                self.previous_response_id = None;
                println!("Conversation cleared.");
            }
            "/context" => {
                let ctx = self.context.as_system_prompt();
                if ctx.is_empty() {
                    println!("No context loaded.");
                } else {
                    println!("{}", ctx);
                }
            }
            "/status" => {
                self.cmd_status().await;
            }
            "/switch" => {
                self.cmd_switch(arg).await;
            }
            "/remember" => {
                if arg.is_empty() {
                    println!("Usage: /remember <text to remember>");
                } else {
                    self.cmd_remember(arg).await;
                }
            }
            "/recall" => {
                if arg.is_empty() {
                    println!("Usage: /recall <search query>");
                } else {
                    self.cmd_recall(arg).await;
                }
            }
            "/tasks" => {
                self.cmd_tasks().await;
            }
            "/quit" | "/exit" => {
                std::process::exit(0);
            }
            _ => {
                println!("Unknown command: {}. Try /help", command);
            }
        }
        Ok(())
    }

    /// /status - Show current state
    async fn cmd_status(&self) {
        println!("Project: {}", self.context.project_path.as_deref().unwrap_or("(none)"));
        println!("Conversation: {}", if self.previous_response_id.is_some() { "active" } else { "new" });
        println!("Semantic search: {}", if self.semantic.is_available() { "enabled" } else { "disabled" });

        if let Some(ref db) = self.db {
            // Count goals
            let goals: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM goals WHERE status = 'in_progress'")
                .fetch_one(db)
                .await
                .unwrap_or((0,));
            println!("Active goals: {}", goals.0);

            // Count tasks
            let tasks: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM tasks WHERE status != 'completed'")
                .fetch_one(db)
                .await
                .unwrap_or((0,));
            println!("Pending tasks: {}", tasks.0);

            // Count memories
            let memories: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM memory_facts")
                .fetch_one(db)
                .await
                .unwrap_or((0,));
            println!("Memories: {}", memories.0);
        }
    }

    /// /switch - Change project
    async fn cmd_switch(&mut self, path: &str) {
        let new_path = if path.is_empty() {
            std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| ".".to_string())
        } else {
            path.to_string()
        };

        // Reload context for new project
        if let Some(ref db) = self.db {
            match crate::context::MiraContext::load(db, &new_path).await {
                Ok(ctx) => {
                    self.context = ctx;
                    println!("Switched to: {}", new_path);
                    println!("  {} corrections, {} goals, {} memories",
                        self.context.corrections.len(),
                        self.context.goals.len(),
                        self.context.memories.len());
                }
                Err(e) => {
                    println!("Failed to load context: {}", e);
                    self.context.project_path = Some(new_path.clone());
                    println!("Switched to: {} (no context)", new_path);
                }
            }
        } else {
            self.context.project_path = Some(new_path.clone());
            println!("Switched to: {} (no database)", new_path);
        }

        // Clear conversation on project switch
        self.previous_response_id = None;
    }

    /// /remember - Store in memory
    async fn cmd_remember(&self, content: &str) {
        use chrono::Utc;
        use uuid::Uuid;

        if let Some(ref db) = self.db {
            let now = Utc::now().timestamp();
            let id = Uuid::new_v4().to_string();
            let key: String = content
                .chars()
                .take(50)
                .collect::<String>()
                .to_lowercase()
                .replace(|c: char| !c.is_alphanumeric() && c != ' ', "")
                .trim()
                .to_string();

            let result = sqlx::query(r#"
                INSERT INTO memory_facts (id, fact_type, key, value, category, source, confidence, times_used, created_at, updated_at)
                VALUES ($1, 'general', $2, $3, NULL, 'mira-chat', 1.0, 0, $4, $4)
                ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at
            "#)
            .bind(&id)
            .bind(&key)
            .bind(content)
            .bind(now)
            .execute(db)
            .await;

            match result {
                Ok(_) => {
                    println!("Remembered: \"{}\"", if content.len() > 50 { &content[..50] } else { content });

                    // Also store in Qdrant if available
                    if self.semantic.is_available() {
                        use std::collections::HashMap;
                        let mut metadata = HashMap::new();
                        metadata.insert("fact_type".into(), serde_json::json!("general"));
                        metadata.insert("key".into(), serde_json::json!(key));

                        if let Err(e) = self.semantic.store(
                            crate::semantic::COLLECTION_MEMORY,
                            &id,
                            content,
                            metadata
                        ).await {
                            println!("  (semantic index failed: {})", e);
                        }
                    }
                }
                Err(e) => println!("Failed to remember: {}", e),
            }
        } else {
            println!("No database connected.");
        }
    }

    /// /recall - Search memory
    async fn cmd_recall(&self, query: &str) {
        // Try semantic search first
        if self.semantic.is_available() {
            match self.semantic.search(crate::semantic::COLLECTION_MEMORY, query, 5, None).await {
                Ok(results) if !results.is_empty() => {
                    println!("Found {} memories (semantic):", results.len());
                    for (i, r) in results.iter().enumerate() {
                        let preview = if r.content.len() > 80 {
                            format!("{}...", &r.content[..80])
                        } else {
                            r.content.clone()
                        };
                        println!("  {}. [score: {:.2}] {}", i + 1, r.score, preview);
                    }
                    return;
                }
                Ok(_) => {} // Fall through to text search
                Err(e) => {
                    println!("  (semantic search failed: {}, trying text...)", e);
                }
            }
        }

        // Fallback to text search
        if let Some(ref db) = self.db {
            let pattern = format!("%{}%", query);
            let rows: Vec<(String, String)> = sqlx::query_as(
                "SELECT key, value FROM memory_facts WHERE value LIKE $1 OR key LIKE $1 ORDER BY times_used DESC LIMIT 5"
            )
            .bind(&pattern)
            .fetch_all(db)
            .await
            .unwrap_or_default();

            if rows.is_empty() {
                println!("No memories found for: {}", query);
            } else {
                println!("Found {} memories (text):", rows.len());
                for (i, (key, value)) in rows.iter().enumerate() {
                    let preview = if value.len() > 80 {
                        format!("{}...", &value[..80])
                    } else {
                        value.clone()
                    };
                    println!("  {}. [{}] {}", i + 1, key, preview);
                }
            }
        } else {
            println!("No database connected.");
        }
    }

    /// /tasks - List tasks
    async fn cmd_tasks(&self) {
        if let Some(ref db) = self.db {
            let rows: Vec<(String, String, String)> = sqlx::query_as(
                "SELECT title, status, priority FROM tasks WHERE status != 'completed' ORDER BY
                 CASE priority WHEN 'urgent' THEN 1 WHEN 'high' THEN 2 WHEN 'medium' THEN 3 ELSE 4 END,
                 created_at DESC LIMIT 10"
            )
            .fetch_all(db)
            .await
            .unwrap_or_default();

            if rows.is_empty() {
                println!("No pending tasks.");
            } else {
                println!("Tasks ({}):", rows.len());
                for (title, status, priority) in rows {
                    let icon = match status.as_str() {
                        "in_progress" => "◐",
                        "blocked" => "✗",
                        _ => "○",
                    };
                    println!("  {} [{}] {}", icon, priority, title);
                }
            }
        } else {
            println!("No database connected.");
        }
    }

    /// Process user input with streaming responses
    async fn process_input_streaming(&mut self, input: &str) -> Result<()> {
        let client = match &self.client {
            Some(c) => c,
            None => {
                eprintln!("Error: API client not initialized");
                return Ok(());
            }
        };

        // Classify task complexity for reasoning effort
        let effort = classify(input);
        let effort_str = effort.as_str();
        println!("  [reasoning: {}]", effort_str);

        // Build system prompt with context
        let system_prompt = build_system_prompt(&self.context);
        let tools = get_tools();

        // Track total usage
        let mut total_usage = Usage {
            input_tokens: 0,
            output_tokens: 0,
            input_tokens_details: None,
            output_tokens_details: None,
        };

        // Initial streaming request
        let mut rx = match client
            .create_stream(
                input,
                &system_prompt,
                self.previous_response_id.as_deref(),
                effort_str,
                &tools,
            )
            .await
        {
            Ok(rx) => rx,
            Err(e) => {
                eprintln!("Error: {}", e);
                return Ok(());
            }
        };

        // Agentic loop
        const MAX_ITERATIONS: usize = 10;
        for iteration in 0..MAX_ITERATIONS {
            // Process streaming events (returns (result, was_cancelled))
            let (stream_result, was_cancelled) = self.process_stream(&mut rx).await?;

            // If cancelled, break out of the loop
            if was_cancelled {
                break;
            }

            // Update response ID
            if let Some(ref resp) = stream_result.final_response {
                self.previous_response_id = Some(resp.id.clone());

                // Accumulate usage
                if let Some(ref usage) = resp.usage {
                    total_usage.input_tokens += usage.input_tokens;
                    total_usage.output_tokens += usage.output_tokens;
                }
            }

            // If no function calls, we're done
            if stream_result.function_calls.is_empty() {
                break;
            }

            // Make sure we have a previous response ID before continuing
            let prev_id = match &self.previous_response_id {
                Some(id) if !id.is_empty() => id.clone(),
                _ => {
                    eprintln!("  [error: no response ID for continuation]");
                    break;
                }
            };

            // Execute function calls
            let mut tool_results: Vec<(String, String)> = Vec::new();
            for (name, call_id, arguments) in &stream_result.function_calls {
                // Check for cancellation before each tool
                if self.is_cancelled() {
                    println!("  [cancelled]");
                    return Ok(());
                }

                println!("  [tool: {}]", name);

                let result = self.tools.execute(name, arguments).await?;
                let result_len = result.len();

                // Truncate for display
                let display_result = if result_len > 200 {
                    format!("{}... ({} bytes)", &result[..200], result_len)
                } else {
                    result.clone()
                };
                println!("  [result: {}]", display_result.trim());

                tool_results.push((call_id.clone(), result));
            }

            // Check for cancellation after tool execution
            if self.is_cancelled() {
                println!("  [cancelled]");
                break;
            }

            // Check iteration limit
            if iteration >= MAX_ITERATIONS - 1 {
                eprintln!("  [warning: max iterations reached]");
                break;
            }

            // Continue with tool results (streaming)
            rx = match client
                .continue_with_tool_results_stream(
                    &prev_id,
                    tool_results,
                    &system_prompt,
                    effort_str,
                    &tools,
                )
                .await
            {
                Ok(rx) => rx,
                Err(e) => {
                    eprintln!("Error continuing: {}", e);
                    break;
                }
            };
        }

        // Show total usage stats
        let cached = total_usage.cached_tokens();
        let cache_pct = if total_usage.input_tokens > 0 {
            (cached as f32 / total_usage.input_tokens as f32) * 100.0
        } else {
            0.0
        };

        let reasoning = total_usage.reasoning_tokens();
        if reasoning > 0 {
            println!(
                "  [tokens: {} in / {} out ({} reasoning), {:.0}% cached]",
                total_usage.input_tokens, total_usage.output_tokens, reasoning, cache_pct
            );
        } else {
            println!(
                "  [tokens: {} in / {} out, {:.0}% cached]",
                total_usage.input_tokens, total_usage.output_tokens, cache_pct
            );
        }

        Ok(())
    }

    /// Process a stream of events, printing text and collecting function calls
    /// Returns (result, was_cancelled)
    async fn process_stream(
        &self,
        rx: &mut mpsc::Receiver<StreamEvent>,
    ) -> Result<(StreamResult, bool)> {
        let mut result = StreamResult::default();
        let mut printed_newline_before = false;
        let mut printed_any_text = false;
        let mut formatter = MarkdownFormatter::new();

        loop {
            // Check for cancellation
            if self.cancelled.load(Ordering::SeqCst) {
                // Flush formatter and reset colors
                let remaining = formatter.flush();
                if !remaining.is_empty() {
                    print!("{}", remaining);
                }
                print!("\x1b[0m"); // Reset any pending colors
                if printed_any_text {
                    println!();
                }
                println!("\n  [cancelled]");
                return Ok((result, true));
            }

            // Use select! to allow cancellation checks even if recv blocks
            tokio::select! {
                event = rx.recv() => {
                    match event {
                        Some(StreamEvent::TextDelta(delta)) => {
                            // Print newline before first text
                            if !printed_newline_before {
                                println!();
                                printed_newline_before = true;
                            }

                            // Format and print delta immediately
                            let formatted = formatter.process(&delta);
                            if !formatted.is_empty() {
                                print!("{}", formatted);
                                io::stdout().flush()?;
                            }
                            printed_any_text = true;
                        }
                        Some(StreamEvent::FunctionCallStart { name, call_id }) => {
                            result.function_calls.push((name, call_id, String::new()));
                        }
                        Some(StreamEvent::FunctionCallDelta { call_id, arguments_delta }) => {
                            // Accumulate arguments
                            if let Some(fc) = result.function_calls.iter_mut().find(|(_, id, _)| id == &call_id) {
                                fc.2.push_str(&arguments_delta);
                            }
                        }
                        Some(StreamEvent::FunctionCallDone { name, call_id, arguments }) => {
                            // Update with final arguments
                            if let Some(fc) = result.function_calls.iter_mut().find(|(_, id, _)| id == &call_id) {
                                fc.2 = arguments;
                            } else {
                                result.function_calls.push((name, call_id, arguments));
                            }
                        }
                        Some(StreamEvent::Done(response)) => {
                            result.final_response = Some(response);
                            break;
                        }
                        Some(StreamEvent::Error(e)) => {
                            eprintln!("\nStream error: {}", e);
                            break;
                        }
                        None => break,
                    }
                }
                // Small timeout to allow cancellation checks
                _ = tokio::time::sleep(tokio::time::Duration::from_millis(50)) => {
                    // Just loop around to check cancellation
                }
            }
        }

        // Flush any remaining formatted content
        let remaining = formatter.flush();
        if !remaining.is_empty() {
            print!("{}", remaining);
            io::stdout().flush()?;
        }

        // Print newline after text if we printed any
        if printed_any_text {
            println!();
            println!();
        }

        Ok((result, false))
    }

    /// Check if operation was cancelled
    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

/// Result of processing a stream
#[derive(Default)]
struct StreamResult {
    /// Collected function calls: (name, call_id, arguments)
    function_calls: Vec<(String, String, String)>,
    /// Final response with usage stats
    final_response: Option<ResponsesResponse>,
}

/// Simple streaming markdown formatter
/// Tracks code block state and applies ANSI colors
struct MarkdownFormatter {
    in_code_block: bool,
    pending: String,
}

impl MarkdownFormatter {
    fn new() -> Self {
        Self {
            in_code_block: false,
            pending: String::new(),
        }
    }

    /// Process a chunk of text and return formatted output
    fn process(&mut self, chunk: &str) -> String {
        // Accumulate chunk with pending content
        self.pending.push_str(chunk);

        let mut output = String::new();
        let mut processed_up_to = 0;

        // Process complete lines and code block markers
        let bytes = self.pending.as_bytes();
        let mut i = 0;

        while i < bytes.len() {
            // Check for code block marker (```)
            if i + 3 <= bytes.len() && &self.pending[i..i + 3] == "```" {
                // Output everything before the marker
                if i > processed_up_to {
                    output.push_str(&self.format_text(&self.pending[processed_up_to..i]));
                }

                // Toggle code block state
                if self.in_code_block {
                    // End of code block - reset color
                    output.push_str("\x1b[0m```");
                    self.in_code_block = false;
                } else {
                    // Start of code block - dim color
                    output.push_str("```\x1b[2m");
                    self.in_code_block = true;
                }

                // Skip to end of line for language specifier
                let mut j = i + 3;
                while j < bytes.len() && bytes[j] != b'\n' {
                    j += 1;
                }
                if j < bytes.len() {
                    output.push_str(&self.pending[i + 3..=j]);
                    processed_up_to = j + 1;
                    i = j + 1;
                } else {
                    // No newline yet, keep pending
                    processed_up_to = i + 3;
                    i = j;
                }
                continue;
            }

            i += 1;
        }

        // Output remaining processed content
        if processed_up_to < self.pending.len() {
            // Check if we might have an incomplete ``` at the end
            let remaining = &self.pending[processed_up_to..];
            let trailing = remaining.len().min(2);
            let safe_len = remaining.len() - trailing;

            if safe_len > 0 {
                output.push_str(&self.format_text(&remaining[..safe_len]));
                self.pending = remaining[safe_len..].to_string();
            } else {
                self.pending = remaining.to_string();
            }
        } else {
            self.pending.clear();
        }

        output
    }

    /// Format text with inline styles (bold, italic)
    fn format_text(&self, text: &str) -> String {
        if self.in_code_block {
            // Inside code block, no inline formatting
            return text.to_string();
        }
        text.to_string()
    }

    /// Flush any remaining pending content
    fn flush(&mut self) -> String {
        if self.pending.is_empty() {
            return String::new();
        }

        let output = self.format_text(&self.pending);
        self.pending.clear();

        // Reset colors if we were in a code block
        if self.in_code_block {
            self.in_code_block = false;
            format!("{}\x1b[0m", output)
        } else {
            output
        }
    }
}

/// Entry point for the REPL with pre-loaded context
pub async fn run_with_context(
    api_key: String,
    context: MiraContext,
    db: Option<SqlitePool>,
    semantic: Arc<SemanticSearch>,
) -> Result<()> {
    let mut repl = Repl::new(db, semantic)?
        .with_api_key(api_key)
        .with_loaded_context(context);

    repl.run().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_repl_new() {
        // Create with no db or semantic
        let semantic = Arc::new(SemanticSearch::new(None, None).await);
        let repl = Repl::new(None, semantic);
        assert!(repl.is_ok());
    }

    #[test]
    fn test_markdown_formatter_plain_text() {
        let mut fmt = MarkdownFormatter::new();
        let out = fmt.process("Hello world");
        // Most text is pending until we're sure there's no ```
        let flush = fmt.flush();
        assert!(out.contains("Hello") || flush.contains("Hello"));
    }

    #[test]
    fn test_markdown_formatter_code_block() {
        let mut fmt = MarkdownFormatter::new();

        // Start code block
        let out1 = fmt.process("```rust\n");
        assert!(out1.contains("```"));
        assert!(out1.contains("\x1b[2m")); // dim color

        // Code content
        let out2 = fmt.process("fn main() {}\n");

        // End code block
        let out3 = fmt.process("```\n");
        assert!(out3.contains("\x1b[0m")); // reset color

        let flush = fmt.flush();
        // Combined output should have code
        let all = format!("{}{}{}{}", out1, out2, out3, flush);
        assert!(all.contains("fn main"));
    }

    #[test]
    fn test_markdown_formatter_flush() {
        let mut fmt = MarkdownFormatter::new();
        let out = fmt.process("partial text here");
        let flush = fmt.flush();
        // Combined output should have the full text
        let all = format!("{}{}", out, flush);
        assert!(all.contains("partial"));
    }
}
