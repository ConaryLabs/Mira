// backend/src/cli/display/terminal.rs
// Terminal display handling with colors and formatting

use console::{style, Style, Term};
use std::io::{self, Write};

/// Color theme for terminal output
#[derive(Debug, Clone)]
pub struct ColorTheme {
    /// User prompt color
    pub prompt: Style,
    /// Assistant response color
    pub assistant: Style,
    /// Thinking/reasoning color
    pub thinking: Style,
    /// Tool name color
    pub tool_name: Style,
    /// Tool success color
    pub tool_success: Style,
    /// Tool error color
    pub tool_error: Style,
    /// Status message color
    pub status: Style,
    /// Error message color
    pub error: Style,
    /// Dim/muted color
    pub dim: Style,
    /// Highlight color
    pub highlight: Style,
    /// Agent color
    pub agent: Style,
}

impl Default for ColorTheme {
    fn default() -> Self {
        Self {
            prompt: Style::new().cyan().bold(),
            assistant: Style::new().white(),
            thinking: Style::new().magenta().dim(),
            tool_name: Style::new().yellow().bold(),
            tool_success: Style::new().green(),
            tool_error: Style::new().red(),
            status: Style::new().blue(),
            error: Style::new().red().bold(),
            dim: Style::new().dim(),
            highlight: Style::new().cyan(),
            agent: Style::new().magenta().bold(),
        }
    }
}

impl ColorTheme {
    /// Create a theme with no colors (for --no-color flag)
    pub fn plain() -> Self {
        Self {
            prompt: Style::new(),
            assistant: Style::new(),
            thinking: Style::new(),
            tool_name: Style::new(),
            tool_success: Style::new(),
            tool_error: Style::new(),
            status: Style::new(),
            error: Style::new(),
            dim: Style::new(),
            highlight: Style::new(),
            agent: Style::new(),
        }
    }
}

/// Terminal display handler
pub struct TerminalDisplay {
    term: Term,
    theme: ColorTheme,
    verbose: bool,
    show_thinking: bool,
    spinner: Option<indicatif::ProgressBar>,
}

impl TerminalDisplay {
    /// Create a new terminal display
    pub fn new(no_color: bool, verbose: bool, show_thinking: bool) -> Self {
        let theme = if no_color {
            ColorTheme::plain()
        } else {
            ColorTheme::default()
        };

        Self {
            term: Term::stdout(),
            theme,
            verbose,
            show_thinking,
            spinner: None,
        }
    }

    /// Print the user prompt indicator
    pub fn print_prompt(&self) -> io::Result<()> {
        print!("{} ", self.theme.prompt.apply_to(">"));
        io::stdout().flush()
    }

    /// Print a welcome message
    pub fn print_welcome(&self) -> io::Result<()> {
        println!();
        println!(
            "{}",
            self.theme.highlight.apply_to("Mira - AI Coding Assistant")
        );
        println!(
            "{}",
            self.theme.dim.apply_to("Type your message and press Enter. Use /help for commands.")
        );
        println!();
        Ok(())
    }

    /// Print assistant response start
    pub fn print_assistant_start(&self) -> io::Result<()> {
        println!();
        print!("{} ", self.theme.dim.apply_to("Mira:"));
        io::stdout().flush()
    }

    /// Print a streaming token
    pub fn print_token(&self, token: &str) -> io::Result<()> {
        print!("{}", self.theme.assistant.apply_to(token));
        io::stdout().flush()
    }

    /// Print end of assistant response
    pub fn print_assistant_end(&self) -> io::Result<()> {
        println!();
        println!();
        Ok(())
    }

    /// Print thinking/reasoning content
    pub fn print_thinking(&self, content: &str) -> io::Result<()> {
        if self.show_thinking {
            println!();
            println!("{}", self.theme.dim.apply_to("Thinking:"));
            for line in content.lines() {
                println!("  {}", self.theme.thinking.apply_to(line));
            }
            println!();
        }
        Ok(())
    }

    /// Print a tool execution
    pub fn print_tool_execution(
        &self,
        tool_name: &str,
        summary: &str,
        success: bool,
        duration_ms: u64,
    ) -> io::Result<()> {
        if self.verbose {
            let status_icon = if success {
                self.theme.tool_success.apply_to("v")
            } else {
                self.theme.tool_error.apply_to("x")
            };

            println!();
            println!(
                "[{}] {} {} {}",
                status_icon,
                self.theme.tool_name.apply_to(tool_name),
                self.theme.dim.apply_to(format!("({}ms)", duration_ms)),
                self.theme.dim.apply_to(summary)
            );
        }
        Ok(())
    }

    /// Print an agent spawn
    pub fn print_agent_spawn(&self, agent_name: &str, task: &str) -> io::Result<()> {
        if self.verbose {
            println!();
            println!(
                "[{}] {}",
                self.theme.agent.apply_to(format!("Spawning: {}", agent_name)),
                self.theme.dim.apply_to(task)
            );
        }
        Ok(())
    }

    /// Print agent progress
    pub fn print_agent_progress(
        &self,
        agent_name: &str,
        iteration: u32,
        max_iterations: u32,
        activity: &str,
    ) -> io::Result<()> {
        if self.verbose {
            println!(
                "  [{}] {}/{}: {}",
                self.theme.agent.apply_to(agent_name),
                iteration,
                max_iterations,
                self.theme.dim.apply_to(activity)
            );
        }
        Ok(())
    }

    /// Print agent completion
    pub fn print_agent_complete(&self, agent_name: &str) -> io::Result<()> {
        if self.verbose {
            println!(
                "[{}] {}",
                self.theme.tool_success.apply_to("v"),
                self.theme.agent.apply_to(format!("{} completed", agent_name))
            );
        }
        Ok(())
    }

    /// Print a status message
    pub fn print_status(&self, message: &str, detail: Option<&str>) -> io::Result<()> {
        if self.verbose {
            print!("{}", self.theme.status.apply_to(message));
            if let Some(d) = detail {
                print!(" {}", self.theme.dim.apply_to(d));
            }
            println!();
        }
        Ok(())
    }

    /// Print an error message
    pub fn print_error(&self, message: &str) -> io::Result<()> {
        eprintln!("{} {}", self.theme.error.apply_to("Error:"), message);
        Ok(())
    }

    /// Print a warning message
    pub fn print_warning(&self, message: &str) -> io::Result<()> {
        eprintln!(
            "{} {}",
            style("Warning:").yellow().bold(),
            message
        );
        Ok(())
    }

    /// Print an info message
    pub fn print_info(&self, message: &str) -> io::Result<()> {
        println!("{} {}", self.theme.status.apply_to("Info:"), message);
        Ok(())
    }

    /// Print a success message
    pub fn print_success(&self, message: &str) -> io::Result<()> {
        println!(
            "{} {}",
            self.theme.tool_success.apply_to("v"),
            message
        );
        Ok(())
    }

    /// Start a spinner with a message
    pub fn start_spinner(&mut self, message: &str) {
        let spinner = indicatif::ProgressBar::new_spinner();
        spinner.set_style(
            indicatif::ProgressStyle::default_spinner()
                .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
                .template("{spinner:.cyan} {msg}")
                .unwrap(),
        );
        spinner.set_message(message.to_string());
        spinner.enable_steady_tick(std::time::Duration::from_millis(80));
        self.spinner = Some(spinner);
    }

    /// Update the spinner message
    pub fn update_spinner(&self, message: &str) {
        if let Some(ref spinner) = self.spinner {
            spinner.set_message(message.to_string());
        }
    }

    /// Stop and clear the spinner
    pub fn stop_spinner(&mut self) {
        if let Some(spinner) = self.spinner.take() {
            spinner.finish_and_clear();
        }
    }

    /// Clear the current line
    pub fn clear_line(&self) -> io::Result<()> {
        self.term.clear_line()?;
        Ok(())
    }

    /// Move cursor up n lines
    pub fn move_up(&self, n: usize) -> io::Result<()> {
        self.term.move_cursor_up(n)?;
        Ok(())
    }

    /// Print a horizontal separator
    pub fn print_separator(&self) -> io::Result<()> {
        let width = self.term.size().1 as usize;
        let line = "─".repeat(width.min(80));
        println!("{}", self.theme.dim.apply_to(line));
        Ok(())
    }

    /// Print help text
    pub fn print_help(&self) -> io::Result<()> {
        println!();
        println!("{}", self.theme.highlight.apply_to("Available Commands:"));
        println!();
        println!("  {}  Show this help message", self.theme.tool_name.apply_to("/help"));
        println!("  {}  Clear the screen", self.theme.tool_name.apply_to("/clear"));
        println!("  {}  List available slash commands", self.theme.tool_name.apply_to("/commands"));
        println!("  {}  List checkpoints", self.theme.tool_name.apply_to("/checkpoints"));
        println!("  {}  Rewind to a checkpoint", self.theme.tool_name.apply_to("/rewind <id>"));
        println!("  {}  Exit the CLI", self.theme.tool_name.apply_to("/quit"));
        println!();
        println!("{}", self.theme.highlight.apply_to("Keyboard Shortcuts:"));
        println!();
        println!("  {}  Cancel current operation", self.theme.tool_name.apply_to("Ctrl+C"));
        println!("  {}  Exit the CLI", self.theme.tool_name.apply_to("Ctrl+D"));
        println!();
        Ok(())
    }

    /// Get terminal width
    pub fn width(&self) -> usize {
        self.term.size().1 as usize
    }

    /// Print a colored diff
    pub fn print_diff(&self, diff: &str) -> io::Result<()> {
        println!();
        for line in diff.lines() {
            if line.starts_with('+') && !line.starts_with("+++") {
                println!("{}", style(line).green());
            } else if line.starts_with('-') && !line.starts_with("---") {
                println!("{}", style(line).red());
            } else if line.starts_with('@') {
                println!("{}", style(line).cyan());
            } else if line.starts_with("diff") || line.starts_with("index") {
                println!("{}", self.theme.dim.apply_to(line));
            } else {
                println!("{}", line);
            }
        }
        println!();
        Ok(())
    }

    /// Print a task list (todo-like display)
    pub fn print_task_list(&self, tasks: &[(String, bool, Option<String>)]) -> io::Result<()> {
        println!();
        for (title, completed, detail) in tasks {
            let status = if *completed {
                self.theme.tool_success.apply_to("[x]")
            } else {
                self.theme.dim.apply_to("[ ]")
            };
            let title_style = if *completed {
                self.theme.dim.apply_to(title.as_str())
            } else {
                self.theme.assistant.apply_to(title.as_str())
            };
            print!("  {} {}", status, title_style);
            if let Some(d) = detail {
                print!(" {}", self.theme.dim.apply_to(d));
            }
            println!();
        }
        println!();
        Ok(())
    }

    /// Print file content with line numbers
    pub fn print_file_content(&self, path: &str, content: &str, start_line: usize) -> io::Result<()> {
        println!();
        println!("{}", self.theme.tool_name.apply_to(path));
        println!("{}", self.theme.dim.apply_to("─".repeat(60.min(self.width()))));
        for (i, line) in content.lines().enumerate() {
            let line_num = start_line + i;
            println!(
                "{} {}",
                self.theme.dim.apply_to(format!("{:4}│", line_num)),
                line
            );
        }
        println!();
        Ok(())
    }

    /// Print a search result
    pub fn print_search_result(&self, path: &str, line_num: usize, content: &str, match_text: &str) -> io::Result<()> {
        print!(
            "{}{} ",
            self.theme.highlight.apply_to(path),
            self.theme.dim.apply_to(format!(":{}", line_num))
        );
        // Highlight the match within the content
        if let Some(pos) = content.find(match_text) {
            print!("{}", &content[..pos]);
            print!("{}", self.theme.tool_name.apply_to(match_text));
            println!("{}", &content[pos + match_text.len()..]);
        } else {
            println!("{}", content);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_color_theme_default() {
        let theme = ColorTheme::default();
        // Just verify it doesn't panic
        let _ = theme.prompt.apply_to("test");
    }

    #[test]
    fn test_color_theme_plain() {
        let theme = ColorTheme::plain();
        // Plain theme should also work
        let _ = theme.prompt.apply_to("test");
    }

    #[test]
    fn test_terminal_display_creation() {
        let display = TerminalDisplay::new(true, false, false);
        assert!(!display.verbose);
        assert!(!display.show_thinking);
    }
}
