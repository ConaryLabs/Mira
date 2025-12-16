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
use crate::responses::{Client, OutputItem};
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

    /// Load context from Mira
    pub async fn with_context(mut self, db_url: &str, qdrant_url: &str) -> Result<Self> {
        self.context = MiraContext::load(db_url, qdrant_url).await?;
        Ok(self)
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
        println!("  [reasoning: {}]", effort);

        // Build system prompt with context
        let system_prompt = build_system_prompt(&self.context);
        let tools = get_tools();

        // Call GPT-5.2 Responses API
        let response = client
            .create(
                input,
                &system_prompt,
                self.previous_response_id.as_deref(),
                effort.as_str(),
                &tools,
            )
            .await;

        match response {
            Ok(resp) => {
                // Store response ID for conversation continuity
                self.previous_response_id = Some(resp.id.clone());

                // Process output items
                for item in &resp.output {
                    match item {
                        OutputItem::Reasoning { summary } => {
                            println!("  [thinking: {}]", summary);
                        }
                        OutputItem::Message { content } => {
                            println!("\n{}\n", content);
                        }
                        OutputItem::FunctionCall {
                            name,
                            arguments,
                            call_id: _,
                        } => {
                            println!("  [tool: {}]", name);

                            // Execute tool
                            let result = self.tools.execute(name, arguments).await?;

                            // TODO: Send tool result back to API for continuation
                            println!("  [result: {} bytes]", result.len());
                        }
                    }
                }

                // Show usage stats
                if let Some(usage) = &resp.usage {
                    let cache_pct = if usage.input_tokens > 0 {
                        (usage.cached_input_tokens as f32 / usage.input_tokens as f32) * 100.0
                    } else {
                        0.0
                    };
                    println!(
                        "  [tokens: {} in / {} out, {:.0}% cached]",
                        usage.input_tokens, usage.output_tokens, cache_pct
                    );
                }
            }
            Err(e) => {
                eprintln!("Error: {}", e);
            }
        }

        Ok(())
    }
}

/// Entry point for the REPL
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
