// src/lib.rs

pub mod api;
pub mod auth;
pub mod budget;
pub mod cache;
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
pub mod sudo;
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

// Export commonly used items
pub use config::CONFIG;
pub use state::AppState;
