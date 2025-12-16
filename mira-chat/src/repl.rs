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

use crate::context::{build_system_prompt, MiraContext};
use crate::reasoning::classify;
use crate::responses::Client;
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
    pub fn new() -> Result<Self> {
        let editor = DefaultEditor::new()?;

        // History file in ~/.mira/chat_history
        let history_path = dirs::home_dir()
            .unwrap_or_default()
            .join(".mira")
            .join("chat_history");

        Ok(Self {
            editor,
            client: None,
            tools: ToolExecutor::new(),
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
        println!();

        loop {
            let readline = self.editor.readline(">>> ");

            match readline {
                Ok(line) => {
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

                    // Process user input
                    self.process_input(trimmed).await?;
                }
                Err(ReadlineError::Interrupted) => {
                    println!("^C");
                    continue;
                }
                Err(ReadlineError::Eof) => {
                    println!("Goodbye!");
                    break;
                }
                Err(err) => {
                    eprintln!("Error: {:?}", err);
                    break;
                }
            }
        }

        self.save_history();
        Ok(())
    }

    /// Handle slash commands
    async fn handle_command(&mut self, cmd: &str) -> Result<()> {
        match cmd {
            "/help" => {
                println!("Commands:");
                println!("  /help     - Show this help");
                println!("  /clear    - Clear conversation history");
                println!("  /context  - Show current Mira context");
                println!("  /effort   - Show/set reasoning effort");
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

    /// Process user input and get response
    async fn process_input(&mut self, input: &str) -> Result<()> {
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

        // Track total tokens across all turns
        let mut total_input = 0u32;
        let mut total_output = 0u32;
        let mut total_cached = 0u32;
        let mut total_reasoning = 0u32;

        // Initial request
        let response = client
            .create(
                input,
                &system_prompt,
                self.previous_response_id.as_deref(),
                effort_str,
                &tools,
            )
            .await;

        let mut current_response = match response {
            Ok(resp) => resp,
            Err(e) => {
                eprintln!("Error: {}", e);
                return Ok(());
            }
        };

        // Agentic loop - keep going until we get a text response with no tool calls
        const MAX_ITERATIONS: usize = 10;
        for iteration in 0..MAX_ITERATIONS {
            // Store response ID for conversation continuity
            self.previous_response_id = Some(current_response.id.clone());

            // Accumulate usage
            if let Some(usage) = &current_response.usage {
                total_input += usage.input_tokens;
                total_output += usage.output_tokens;
                total_cached += usage.cached_tokens();
                total_reasoning += usage.reasoning_tokens();
            }

            // Collect function calls and execute them
            let mut tool_results: Vec<(String, String)> = Vec::new();
            let mut has_text_output = false;

            for item in &current_response.output {
                // Handle text messages
                if let Some(text) = item.text() {
                    println!("\n{}\n", text);
                    has_text_output = true;
                }

                // Handle function calls
                if let Some((name, arguments, call_id)) = item.as_function_call() {
                    println!("  [tool: {}]", name);

                    // Execute tool
                    let result = self.tools.execute(name, arguments).await?;
                    let result_len = result.len();

                    // Truncate for display
                    let display_result = if result_len > 200 {
                        format!("{}... ({} bytes total)", &result[..200], result_len)
                    } else {
                        result.clone()
                    };
                    println!("  [result: {}]", display_result.trim());

                    tool_results.push((call_id.to_string(), result));
                }
            }

            // If no tool calls, we're done
            if tool_results.is_empty() {
                break;
            }

            // If we have tool results, send them back
            if iteration >= MAX_ITERATIONS - 1 {
                eprintln!("  [warning: max iterations reached]");
                break;
            }

            let continuation = client
                .continue_with_tool_results(
                    &current_response.id,
                    tool_results,
                    &system_prompt,
                    effort_str,
                    &tools,
                )
                .await;

            current_response = match continuation {
                Ok(resp) => resp,
                Err(e) => {
                    eprintln!("Error continuing: {}", e);
                    break;
                }
            };
        }

        // Show total usage stats
        let cache_pct = if total_input > 0 {
            (total_cached as f32 / total_input as f32) * 100.0
        } else {
            0.0
        };

        if total_reasoning > 0 {
            println!(
                "  [tokens: {} in / {} out ({} reasoning), {:.0}% cached]",
                total_input, total_output, total_reasoning, cache_pct
            );
        } else {
            println!(
                "  [tokens: {} in / {} out, {:.0}% cached]",
                total_input, total_output, cache_pct
            );
        }

        Ok(())
    }
}

/// Entry point for the REPL with pre-loaded context
pub async fn run_with_context(api_key: String, context: MiraContext) -> Result<()> {
    let mut repl = Repl::new()?
        .with_api_key(api_key)
        .with_loaded_context(context);

    repl.run().await
}

/// Entry point for the REPL (loads context itself)
pub async fn run() -> Result<()> {
    let api_key = std::env::var("OPENAI_API_KEY")
        .expect("OPENAI_API_KEY required");

    let mut repl = Repl::new()?
        .with_api_key(api_key);

    repl.run().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repl_new() {
        let repl = Repl::new();
        assert!(repl.is_ok());
    }
}
