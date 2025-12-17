//! Interactive REPL for Mira Chat
//!
//! Provides a readline-based interface with:
//! - Command history
//! - Multi-line input support
//! - Streaming response display
//! - Tool execution feedback

mod formatter;
mod helper;

use anyhow::Result;
use rustyline::error::ReadlineError;
use rustyline::history::DefaultHistory;
use rustyline::Editor;
use sqlx::SqlitePool;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;

use crate::context::{build_system_prompt, MiraContext};
use crate::reasoning::classify;
use crate::responses::{Client, ResponsesResponse, StreamEvent, Usage};
use crate::semantic::SemanticSearch;
use crate::session::SessionManager;
use crate::tools::{get_tools, ToolExecutor};

use formatter::MarkdownFormatter;
use helper::MiraHelper;

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
        lines.push(
            first_line
                .trim()
                .strip_suffix('\\')
                .unwrap_or(first_line.trim())
                .to_string(),
        );

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
            "/version" => {
                println!("Mira Chat v{}", env!("CARGO_PKG_VERSION"));
                println!("  Model: GPT-5.2 Thinking");
                println!("  Backend: Mira Power Suit");
            }
            "/uptime" => {
                let elapsed = self.start_time.elapsed();
                println!("Uptime: {}", Self::format_duration(elapsed));
            }

            "/help" => {
                println!("Commands:");
                println!("  /help              - Show this help");
                println!("  /version           - Show version info");
                println!("  /uptime            - Show session uptime");
                println!("  /clear             - Clear conversation history");
                println!("  /compact           - Compact code context");
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
                // Also clear session if available
                if let Some(ref session) = self.session {
                    if let Err(e) = session.clear_conversation().await {
                        println!("Warning: failed to clear session: {}", e);
                    }
                }
                println!("Conversation cleared.");
            }
            "/compact" => {
                self.cmd_compact().await;
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

    fn format_duration(d: std::time::Duration) -> String {
        let mut secs = d.as_secs();

        let days = secs / 86_400;
        secs %= 86_400;
        let hours = secs / 3_600;
        secs %= 3_600;
        let mins = secs / 60;
        secs %= 60;

        if days > 0 {
            format!("{}d {}h {}m {}s", days, hours, mins, secs)
        } else if hours > 0 {
            format!("{}h {}m {}s", hours, mins, secs)
        } else if mins > 0 {
            format!("{}m {}s", mins, secs)
        } else {
            format!("{}s", secs)
        }
    }

    /// /status - Show current state
    async fn cmd_status(&self) {
        println!(
            "Project: {}",
            self.context.project_path.as_deref().unwrap_or("(none)")
        );
        println!(
            "Semantic search: {}",
            if self.semantic.is_available() {
                "enabled"
            } else {
                "disabled"
            }
        );

        // Show session info
        if let Some(ref session) = self.session {
            if let Ok(stats) = session.stats().await {
                println!(
                    "Session: {} messages, {} summaries{}{}",
                    stats.total_messages,
                    stats.summary_count,
                    if stats.has_active_conversation {
                        ", active"
                    } else {
                        ""
                    },
                    if stats.has_code_compaction {
                        ", has compaction"
                    } else {
                        ""
                    }
                );
            }
        } else {
            println!("Session: disabled");
        }

        if let Some(ref db) = self.db {
            // Count goals
            let goals: (i64,) =
                sqlx::query_as("SELECT COUNT(*) FROM goals WHERE status = 'in_progress'")
                    .fetch_one(db)
                    .await
                    .unwrap_or((0,));
            println!("Active goals: {}", goals.0);

            // Count tasks
            let tasks: (i64,) =
                sqlx::query_as("SELECT COUNT(*) FROM tasks WHERE status != 'completed'")
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

    /// /compact - Compact code context into encrypted blob
    async fn cmd_compact(&self) {
        let session = match &self.session {
            Some(s) => s,
            None => {
                println!("Session not available for compaction.");
                return;
            }
        };

        let client = match &self.client {
            Some(c) => c,
            None => {
                println!("API client not available.");
                return;
            }
        };

        // Get touched files
        let files = session.get_touched_files();
        if files.is_empty() {
            println!("No files touched in this session.");
            return;
        }

        // Get response ID for compaction
        let response_id = match session.get_response_id().await {
            Ok(Some(id)) => id,
            _ => {
                println!("No active conversation to compact.");
                return;
            }
        };

        println!("Compacting {} file(s)...", files.len());
        for f in &files {
            println!("  - {}", f);
        }

        // Build context description
        let context = format!(
            "Code context for project. Files touched: {}",
            files.join(", ")
        );

        // Call compaction endpoint
        match client.compact(&response_id, &context).await {
            Ok(response) => {
                // Store the compaction blob
                if let Err(e) = session.store_compaction(&response.encrypted_content, &files).await
                {
                    println!("Failed to store compaction: {}", e);
                    return;
                }

                // Clear touched files
                session.clear_touched_files();

                let saved = response.tokens_saved.unwrap_or(0);
                println!("Compacted! {} tokens saved.", saved);
            }
            Err(e) => {
                println!("Compaction failed: {}", e);
            }
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
                    println!(
                        "  {} corrections, {} goals, {} memories",
                        self.context.corrections.len(),
                        self.context.goals.len(),
                        self.context.memories.len()
                    );
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

            let result = sqlx::query(
                r#"
                INSERT INTO memory_facts (id, fact_type, key, value, category, source, confidence, times_used, created_at, updated_at)
                VALUES ($1, 'general', $2, $3, NULL, 'mira-chat', 1.0, 0, $4, $4)
                ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at
            "#,
            )
            .bind(&id)
            .bind(&key)
            .bind(content)
            .bind(now)
            .execute(db)
            .await;

            match result {
                Ok(_) => {
                    println!(
                        "Remembered: \"{}\"",
                        if content.len() > 50 {
                            &content[..50]
                        } else {
                            content
                        }
                    );

                    // Also store in Qdrant if available
                    if self.semantic.is_available() {
                        use std::collections::HashMap;
                        let mut metadata = HashMap::new();
                        metadata.insert("fact_type".into(), serde_json::json!("general"));
                        metadata.insert("key".into(), serde_json::json!(key));

                        if let Err(e) = self
                            .semantic
                            .store(crate::semantic::COLLECTION_MEMORY, &id, content, metadata)
                            .await
                        {
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
            match self
                .semantic
                .search(crate::semantic::COLLECTION_MEMORY, query, 5, None)
                .await
            {
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
                 created_at DESC LIMIT 10",
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

        // Save user message to session (for invisible persistence)
        if let Some(ref session) = self.session {
            if let Err(e) = session.save_message("user", input).await {
                tracing::debug!("Failed to save user message: {}", e);
            }
        }

        // Classify task complexity for reasoning effort
        let effort = classify(input);
        let effort_str = effort.as_str();
        println!("  [reasoning: {}]", effort_str);

        // Assemble context using session manager (or fallback to static context)
        //
        // CACHE OPTIMIZATION: Prompt is structured for maximum LLM cache hits.
        // Order from most stable (first) to least stable (last):
        //   1. Base instructions (static, never changes)
        //   2. Project path (stable within session)
        //   3. Corrections, goals, memories (change occasionally)
        //   4. Compaction blob (changes on compaction)
        //   5. Summaries (changes on summarization)
        //   6. Semantic context (changes per query)
        //
        // This ensures the longest possible prefix match for caching.
        let (system_prompt, prev_response_id) = if let Some(ref session) = self.session {
            match session.assemble_context(input).await {
                Ok(assembled) => {
                    // Build full system prompt with assembled context
                    // base_prompt = instructions + project + corrections/goals/memories
                    // extra_context = compaction + summaries + semantic (cache-ordered)
                    let base_prompt = build_system_prompt(&assembled.mira_context);
                    let extra_context = assembled.format_for_prompt();
                    let full_prompt = if extra_context.is_empty() {
                        base_prompt
                    } else {
                        format!("{}\n\n{}", base_prompt, extra_context)
                    };
                    (full_prompt, assembled.previous_response_id)
                }
                Err(e) => {
                    tracing::warn!("Failed to assemble context: {}", e);
                    (
                        build_system_prompt(&self.context),
                        self.previous_response_id.clone(),
                    )
                }
            }
        } else {
            (
                build_system_prompt(&self.context),
                self.previous_response_id.clone(),
            )
        };

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
                prev_response_id.as_deref(),
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

        // Agentic loop - track assistant response text for saving
        let mut full_response_text = String::new();
        const MAX_ITERATIONS: usize = 10;
        for iteration in 0..MAX_ITERATIONS {
            // Process streaming events (returns (result, was_cancelled, response_text))
            let (stream_result, was_cancelled, response_text) =
                self.process_stream(&mut rx).await?;
            full_response_text.push_str(&response_text);

            // If cancelled, break out of the loop
            if was_cancelled {
                break;
            }

            // Update response ID
            if let Some(ref resp) = stream_result.final_response {
                self.previous_response_id = Some(resp.id.clone());

                // Save response ID to session for persistence
                if let Some(ref session) = self.session {
                    if let Err(e) = session.set_response_id(&resp.id).await {
                        tracing::debug!("Failed to save response ID: {}", e);
                    }
                }

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

            // Execute function calls in parallel for efficiency
            let num_calls = stream_result.function_calls.len();
            if num_calls > 1 {
                println!("  [executing {} tools in parallel]", num_calls);
            }

            // Check for cancellation before starting
            if self.is_cancelled() {
                println!("  [cancelled]");
                return Ok(());
            }

            // Create futures for all tool calls
            let tool_futures: Vec<_> = stream_result
                .function_calls
                .iter()
                .map(|(name, call_id, arguments)| {
                    let executor = self.tools.clone();
                    let name = name.clone();
                    let call_id = call_id.clone();
                    let arguments = arguments.clone();
                    async move {
                        let result = executor.execute(&name, &arguments).await;
                        (name, call_id, result)
                    }
                })
                .collect();

            // Execute all in parallel
            let results = futures::future::join_all(tool_futures).await;

            // Process results
            let mut tool_results: Vec<(String, String)> = Vec::new();
            for (name, call_id, result) in results {
                let result = result?;
                let result_len = result.len();

                // Truncate for display
                let display_result = if result_len > 200 {
                    format!("{}... ({} bytes)", &result[..200], result_len)
                } else {
                    result.clone()
                };
                println!("  [tool: {}] -> {}", name, display_result.trim());

                tool_results.push((call_id, result));
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

        // Save assistant response to session (for invisible persistence)
        if !full_response_text.is_empty() {
            if let Some(ref session) = self.session {
                if let Err(e) = session.save_message("assistant", &full_response_text).await {
                    tracing::debug!("Failed to save assistant message: {}", e);
                }
            }
        }

        // Check if summarization is needed and do it
        if let Some(ref session) = self.session {
            if let Ok(Some(messages_to_summarize)) = session.check_summarization_needed().await {
                println!(
                    "  [summarizing {} old messages...]",
                    messages_to_summarize.len()
                );

                // Format messages for summarization API
                let formatted: Vec<(String, String)> = messages_to_summarize
                    .iter()
                    .map(|m| (m.role.clone(), m.content.clone()))
                    .collect();

                // Call GPT to summarize
                if let Ok(summary) = client.summarize_messages(&formatted).await {
                    // Collect message IDs
                    let ids: Vec<String> =
                        messages_to_summarize.iter().map(|m| m.id.clone()).collect();

                    // Store summary and delete old messages
                    if let Err(e) = session.store_summary(&summary, &ids).await {
                        tracing::warn!("Failed to store summary: {}", e);
                    } else {
                        println!("  [compressed to summary]");
                    }
                } else {
                    tracing::debug!("Summarization API call failed, will retry later");
                }
            }
        }

        // Auto-compact code context when enough files touched
        const AUTO_COMPACT_THRESHOLD: usize = 10;
        if let Some(ref session) = self.session {
            let touched_files = session.get_touched_files();
            if touched_files.len() >= AUTO_COMPACT_THRESHOLD {
                if let Ok(Some(response_id)) = session.get_response_id().await {
                    println!("  [auto-compacting {} files...]", touched_files.len());

                    let context = format!(
                        "Code context for project. Files: {}",
                        touched_files
                            .iter()
                            .take(20)
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(", ")
                    );

                    match client.compact(&response_id, &context).await {
                        Ok(response) => {
                            if let Err(e) = session
                                .store_compaction(&response.encrypted_content, &touched_files)
                                .await
                            {
                                tracing::warn!("Failed to store compaction: {}", e);
                            } else {
                                session.clear_touched_files();
                                let saved = response.tokens_saved.unwrap_or(0);
                                println!("  [compacted, {} tokens saved]", saved);
                            }
                        }
                        Err(e) => {
                            tracing::debug!("Auto-compaction failed: {}", e);
                        }
                    }
                }
            }
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
    /// Returns (result, was_cancelled, accumulated_text)
    async fn process_stream(
        &self,
        rx: &mut mpsc::Receiver<StreamEvent>,
    ) -> Result<(StreamResult, bool, String)> {
        let mut result = StreamResult::default();
        let mut printed_newline_before = false;
        let mut printed_any_text = false;
        let mut formatter = MarkdownFormatter::new();
        let mut accumulated_text = String::new();

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
                return Ok((result, true, accumulated_text));
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

                            // Accumulate raw text for saving
                            accumulated_text.push_str(&delta);

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

        Ok((result, false, accumulated_text))
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
