//! ANSI color helpers for pretty terminal output
//!
//! Simple, tasteful colors that work on most terminals.

/// ANSI escape codes
pub mod ansi {
    pub const RESET: &str = "\x1b[0m";
    pub const BOLD: &str = "\x1b[1m";
    pub const DIM: &str = "\x1b[2m";
    pub const ITALIC: &str = "\x1b[3m";

    // Colors
    pub const RED: &str = "\x1b[31m";
    pub const GREEN: &str = "\x1b[32m";
    pub const YELLOW: &str = "\x1b[33m";
    pub const BLUE: &str = "\x1b[34m";
    pub const MAGENTA: &str = "\x1b[35m";
    pub const CYAN: &str = "\x1b[36m";
    pub const WHITE: &str = "\x1b[37m";
    pub const GRAY: &str = "\x1b[90m";

    // Bright variants
    pub const BRIGHT_GREEN: &str = "\x1b[92m";
    pub const BRIGHT_CYAN: &str = "\x1b[96m";
}

use ansi::*;

/// Format a tool name (cyan, bold)
pub fn tool_name(name: &str) -> String {
    format!("{}{}{}{}", BOLD, CYAN, name, RESET)
}

/// Format a tool result preview (dim)
pub fn tool_result(result: &str) -> String {
    format!("{}{}{}", DIM, result, RESET)
}

/// Format a success message (green)
pub fn success(msg: &str) -> String {
    format!("{}{}{}", GREEN, msg, RESET)
}

/// Format an error message (red)
pub fn error(msg: &str) -> String {
    format!("{}{}{}", RED, msg, RESET)
}

/// Format a warning message (yellow)
pub fn warning(msg: &str) -> String {
    format!("{}{}{}", YELLOW, msg, RESET)
}

/// Format a status/info message (gray/dim)
pub fn status(msg: &str) -> String {
    format!("{}{}{}", GRAY, msg, RESET)
}

/// Format a file path (blue)
pub fn file_path(path: &str) -> String {
    format!("{}{}{}", BLUE, path, RESET)
}

/// Format a header (bold)
pub fn header(msg: &str) -> String {
    format!("{}{}{}", BOLD, msg, RESET)
}

/// Format the prompt
pub fn prompt() -> String {
    format!("{}{}>>> {}", BOLD, MAGENTA, RESET)
}

/// Format the continuation prompt
pub fn continuation_prompt() -> String {
    format!("{}{}... {}", BOLD, MAGENTA, RESET)
}

/// Format reasoning effort indicator
pub fn reasoning(effort: &str) -> String {
    let color = match effort {
        "xhigh" | "high" => YELLOW,
        "medium" => CYAN,
        "low" => GRAY,
        _ => DIM,
    };
    format!("{}[reasoning: {}]{}", color, effort, RESET)
}

/// Format token usage
pub fn tokens(input: u64, output: u64, cached_pct: Option<u64>) -> String {
    let cache_str = cached_pct
        .map(|p| format!(", {}% cached", p))
        .unwrap_or_default();
    format!(
        "{}[tokens: {} in / {} out{}]{}",
        DIM, input, output, cache_str, RESET
    )
}

/// Format a horizontal separator
pub fn separator(width: usize) -> String {
    format!("{}{}{}", DIM, "─".repeat(width), RESET)
}

/// Format startup banner line
pub fn banner_line(label: &str, value: &str) -> String {
    format!("{}{:<12}{} {}", DIM, label, RESET, value)
}

/// Format startup banner with accent
pub fn banner_accent(text: &str) -> String {
    format!("{}{}{}{}", BOLD, MAGENTA, text, RESET)
}
