// backend/src/cli/mod.rs
// Mira CLI module - provides a Claude Code-style command line interface

pub mod args;
pub mod config;
pub mod display;
pub mod project;
pub mod repl;
pub mod session;
pub mod ws_client;

// Re-export commonly used items
pub use args::{CliArgs, OutputFormat};
pub use config::CliConfig;
pub use project::{DetectedProject, ProjectDetector};
pub use repl::Repl;
pub use session::{CliSession, SessionStore};
pub use ws_client::MiraClient;
