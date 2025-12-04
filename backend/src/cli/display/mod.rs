// backend/src/cli/display/mod.rs
// Display module for CLI terminal output

mod streaming;
mod terminal;

pub use streaming::StreamingDisplay;
pub use terminal::{TerminalDisplay, ColorTheme};
