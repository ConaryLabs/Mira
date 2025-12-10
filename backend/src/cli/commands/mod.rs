// backend/src/cli/commands/mod.rs
// Slash commands module for CLI

pub mod builtin;
pub mod loader;

pub use builtin::{AgentAction, AgentInfo, BuiltinCommand, ReviewTarget, SearchResult};
pub use loader::CommandLoader;
