// src/lib.rs

pub mod api;
pub mod config;
pub mod llm;
pub mod memory;
pub mod persona;
pub mod prompt;
pub mod project;
pub mod git;
pub mod state;
pub mod utils;
pub mod tasks;
pub mod tools;
pub mod file_system;

// Phase 2 - Core type definitions (operations & relationship systems)
pub mod operations;
pub mod relationship;

// Export commonly used items
pub use state::AppState;
pub use config::CONFIG;
