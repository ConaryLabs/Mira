//! Rustyline helper for REPL with tab completion and hints

use rustyline::completion::{Completer, Pair};
use rustyline::highlight::Highlighter;
use rustyline::hint::{Hinter, HistoryHinter};
use rustyline::validate::Validator;
use rustyline::{Context, Helper};
use std::borrow::Cow;

/// Slash commands for tab completion
pub const SLASH_COMMANDS: &[&str] = &[
    "/help",
    "/version",
    "/uptime",
    "/compact",
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
pub struct MiraHelper {
    hinter: HistoryHinter,
}

impl MiraHelper {
    pub fn new() -> Self {
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
        // Dim hints
        Cow::Owned(format!("\x1b[2m{}\x1b[0m", hint))
    }
}

impl Validator for MiraHelper {}

impl Helper for MiraHelper {}
