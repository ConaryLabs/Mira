//! Interactive REPL for Mira Chat
//!
//! Provides a readline-based interface with:
//! - Command history
//! - Multi-line input support
//! - Streaming response display
//! - Tool execution feedback

use anyhow::Result;
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use sqlx::SqlitePool;
use std::io::{self, Write};
use std::sync::Arc;

use crate::context::{build_system_prompt, MiraContext};
use crate::reasoning::classify;
use crate::responses::{Client, ResponsesResponse, StreamEvent, Tool, Usage};
use crate::semantic::SemanticSearch;
use crate::tools::{get_tools, ToolExecutor};

/// REPL state
pub struct Repl {
    /// Readline editor with history
    editor: DefaultEditor,
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
}

impl Repl {
    pub fn new(db: Option<SqlitePool>, semantic: Arc<SemanticSearch>) -> Result<Self> {
        let editor = DefaultEditor::new()?;

        // History file in ~/.mira/chat_history
        let history_path = dirs::home_dir()
            .unwrap_or_default()
            .join(".mira")
            .join("chat_history");

        // Build ToolExecutor with db and semantic if available
        let mut tools = ToolExecutor::new().with_semantic(semantic);
        if let Some(pool) = db {
            tools = tools.with_db(pool);
        }

        Ok(Self {
            editor,
            client: None,
            tools,
            context: MiraContext::default(),
            previous_response_id: None,
            history_path,
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

        println!("Type your message (Ctrl+D to exit, /help for commands)");
        println!("  Use \\\\ at end of line for multi-line input, or \"\"\" to start/end block");
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
        match cmd {
            "/help" => {
                println!("Commands:");
                println!("  /help     - Show this help");
                println!("  /clear    - Clear conversation history");
                println!("  /context  - Show current Mira context");
                println!("  /quit     - Exit");
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
            "/quit" | "/exit" => {
                std::process::exit(0);
            }
            _ => {
                println!("Unknown command: {}", cmd);
            }
        }
        Ok(())
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
            // Process streaming events
            let stream_result = self.process_stream(&mut rx).await?;

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
    async fn process_stream(
        &self,
        rx: &mut tokio::sync::mpsc::Receiver<StreamEvent>,
    ) -> Result<StreamResult> {
        let mut result = StreamResult::default();
        let mut printed_newline_before = false;
        let mut printed_any_text = false;

        while let Some(event) = rx.recv().await {
            match event {
                StreamEvent::TextDelta(delta) => {
                    // Print newline before first text
                    if !printed_newline_before {
                        println!();
                        printed_newline_before = true;
                    }

                    // Print delta immediately
                    print!("{}", delta);
                    io::stdout().flush()?;
                    printed_any_text = true;
                }
                StreamEvent::FunctionCallStart { name, call_id } => {
                    result.function_calls.push((name, call_id, String::new()));
                }
                StreamEvent::FunctionCallDelta { call_id, arguments_delta } => {
                    // Accumulate arguments
                    if let Some(fc) = result.function_calls.iter_mut().find(|(_, id, _)| id == &call_id) {
                        fc.2.push_str(&arguments_delta);
                    }
                }
                StreamEvent::FunctionCallDone { name, call_id, arguments } => {
                    // Update with final arguments
                    if let Some(fc) = result.function_calls.iter_mut().find(|(_, id, _)| id == &call_id) {
                        fc.2 = arguments;
                    } else {
                        result.function_calls.push((name, call_id, arguments));
                    }
                }
                StreamEvent::Done(response) => {
                    result.final_response = Some(response);
                    break;
                }
                StreamEvent::Error(e) => {
                    eprintln!("\nStream error: {}", e);
                    break;
                }
            }
        }

        // Print newline after text if we printed any
        if printed_any_text {
            println!();
            println!();
        }

        Ok(result)
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
}
