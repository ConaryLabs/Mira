// src/lib.rs

pub mod api;
pub mod config;
pub mod file_system;
pub mod git;
pub mod terminal;
pub mod llm;
pub mod memory;
pub mod persona;
pub mod project;
pub mod prompt;
pub mod state;
pub mod tasks;
pub mod tools;
pub mod utils;

// Phase 2 - Core type definitions (operations & relationship systems)
pub mod operations;
pub mod relationship;

// Export commonly used items
pub use config::CONFIG;
pub use state::AppState;
