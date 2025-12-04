// backend/src/cli/args.rs
// CLI argument definitions using clap

use clap::{Parser, ValueEnum};
use std::path::PathBuf;

/// Mira AI coding assistant CLI
#[derive(Parser, Debug)]
#[command(name = "mira")]
#[command(author = "Conary Labs")]
#[command(version)]
#[command(about = "AI-powered coding assistant", long_about = None)]
pub struct CliArgs {
    /// Prompt to send (use -- before the prompt if it contains flags)
    #[arg()]
    pub prompt: Vec<String>,

    /// Print mode - non-interactive, outputs result and exits
    #[arg(short = 'p', long)]
    pub print: bool,

    /// Continue the most recent conversation
    #[arg(short = 'c', long)]
    pub continue_session: bool,

    /// Resume a specific session by ID, or show picker if no ID given
    #[arg(short = 'r', long)]
    pub resume: Option<Option<String>>,

    /// Output format for responses
    #[arg(long, default_value = "text", value_enum)]
    pub output_format: OutputFormat,

    /// Enable verbose output (show tool executions, reasoning)
    #[arg(short, long)]
    pub verbose: bool,

    /// Backend WebSocket URL
    #[arg(long, env = "MIRA_BACKEND_URL", default_value = "ws://localhost:3001/ws")]
    pub backend_url: String,

    /// Project root directory (auto-detected if not specified)
    #[arg(long)]
    pub project: Option<PathBuf>,

    /// Override system prompt
    #[arg(long)]
    pub system_prompt: Option<String>,

    /// Append to system prompt (preserves default prompt)
    #[arg(long)]
    pub append_system_prompt: Option<String>,

    /// Filter available tools (comma-separated list)
    #[arg(long, value_delimiter = ',')]
    pub tools: Option<Vec<String>>,

    /// Fork from an existing session ID
    #[arg(long)]
    pub fork: Option<String>,

    /// Show thinking/reasoning tokens in output
    #[arg(long)]
    pub show_thinking: bool,

    /// Maximum turns for non-interactive mode
    #[arg(long, default_value = "10")]
    pub max_turns: u32,

    /// Disable colors in output
    #[arg(long)]
    pub no_color: bool,
}

/// Output format for CLI responses
#[derive(Debug, Clone, Copy, ValueEnum, Default, PartialEq)]
pub enum OutputFormat {
    /// Human-readable text with colors and formatting
    #[default]
    Text,
    /// Structured JSON output
    Json,
    /// Newline-delimited JSON (streaming)
    StreamJson,
}

impl CliArgs {
    /// Get the combined prompt from all positional arguments
    pub fn get_prompt(&self) -> Option<String> {
        if self.prompt.is_empty() {
            None
        } else {
            Some(self.prompt.join(" "))
        }
    }

    /// Check if running in non-interactive mode
    pub fn is_non_interactive(&self) -> bool {
        self.print && self.get_prompt().is_some()
    }

    /// Check if we should show the session picker
    pub fn should_show_picker(&self) -> bool {
        matches!(self.resume, Some(None))
    }

    /// Get the session ID to resume, if specified
    pub fn get_resume_session_id(&self) -> Option<&str> {
        match &self.resume {
            Some(Some(id)) => Some(id.as_str()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_args() {
        let args = CliArgs::parse_from(["mira"]);
        assert!(args.prompt.is_empty());
        assert!(!args.print);
        assert!(!args.continue_session);
        assert_eq!(args.output_format, OutputFormat::Text);
    }

    #[test]
    fn test_prompt_with_print() {
        let args = CliArgs::parse_from(["mira", "-p", "fix", "the", "bug"]);
        assert!(args.print);
        assert_eq!(args.get_prompt(), Some("fix the bug".to_string()));
        assert!(args.is_non_interactive());
    }

    #[test]
    fn test_continue_session() {
        let args = CliArgs::parse_from(["mira", "-c"]);
        assert!(args.continue_session);
    }

    #[test]
    fn test_resume_with_id() {
        let args = CliArgs::parse_from(["mira", "-r", "session-123"]);
        assert_eq!(args.get_resume_session_id(), Some("session-123"));
    }

    #[test]
    fn test_output_formats() {
        let args = CliArgs::parse_from(["mira", "--output-format", "json"]);
        assert_eq!(args.output_format, OutputFormat::Json);

        let args = CliArgs::parse_from(["mira", "--output-format", "stream-json"]);
        assert_eq!(args.output_format, OutputFormat::StreamJson);
    }

    #[test]
    fn test_tools_filter() {
        let args = CliArgs::parse_from(["mira", "--tools", "read,write,bash"]);
        assert_eq!(
            args.tools,
            Some(vec![
                "read".to_string(),
                "write".to_string(),
                "bash".to_string()
            ])
        );
    }
}
