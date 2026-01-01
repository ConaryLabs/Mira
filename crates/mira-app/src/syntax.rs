// crates/mira-app/src/syntax.rs
// Syntax highlighting using syntect (pure Rust)

use std::sync::OnceLock;
use syntect::parsing::SyntaxSet;
use syntect::highlighting::ThemeSet;
use syntect::html::{styled_line_to_highlighted_html, IncludeBackground};
use syntect::easy::HighlightLines;

static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
static THEME_SET: OnceLock<ThemeSet> = OnceLock::new();

fn get_syntax_set() -> &'static SyntaxSet {
    SYNTAX_SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn get_theme_set() -> &'static ThemeSet {
    THEME_SET.get_or_init(ThemeSet::load_defaults)
}

/// Highlight a line of code, returning HTML with inline styles
pub fn highlight_line(code: &str, extension: &str) -> String {
    let ss = get_syntax_set();
    let ts = get_theme_set();

    let syntax = ss.find_syntax_by_extension(extension)
        .unwrap_or_else(|| ss.find_syntax_plain_text());

    // Use a dark theme that matches our UI
    let theme = ts.themes.get("base16-ocean.dark")
        .or_else(|| ts.themes.get("InspiredGitHub"))
        .unwrap_or_else(|| ts.themes.values().next().unwrap());

    let mut highlighter = HighlightLines::new(syntax, theme);

    match highlighter.highlight_line(code, ss) {
        Ok(ranges) => styled_line_to_highlighted_html(&ranges[..], IncludeBackground::No)
            .unwrap_or_else(|_| html_escape(code)),
        Err(_) => html_escape(code),
    }
}

pub fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Get file extension from path
pub fn get_extension(path: &str) -> &str {
    path.rsplit('.').next().unwrap_or("")
}
