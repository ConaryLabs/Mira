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
pub mod file_system;  // NEW: Phase 3 - File operations with history tracking

// Export commonly used items
pub use state::AppState;
pub use config::CONFIG;
