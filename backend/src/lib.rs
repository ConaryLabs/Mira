// src/lib.rs

pub mod api;
pub mod auth;
pub mod budget;
pub mod cache;
pub mod checkpoint;
pub mod commands;
pub mod config;
pub mod file_system;
pub mod git;
pub mod hooks;
pub mod llm;
pub mod mcp;
pub mod memory;
pub mod persona;
pub mod project;
pub mod prompt;
pub mod state;
pub mod sudo;
pub mod system;
pub mod tasks;
pub mod tools;
pub mod utils;

// Phase 2 - Core type definitions (operations & relationship systems)
pub mod operations;
pub mod relationship;

// Milestone 4 - Tool synthesis system
pub mod synthesis;

// Milestone 5 - Build system integration
pub mod build;

// Milestone 6 - Reasoning pattern learning
pub mod patterns;

// Milestone 7 - Context Oracle (unified intelligence gathering)
pub mod context_oracle;

// Milestone 8 - Real-time file watching
pub mod watcher;

// Milestone 12 - Agent system (Claude Code-style specialized agents)
pub mod agents;

// CLI module (for mira binary)
pub mod cli;

// Export commonly used items
pub use config::CONFIG;
pub use state::AppState;
