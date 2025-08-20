// src/lib.rs

pub mod api;
pub mod config;  // ADD THIS LINE - centralized configuration
pub mod handlers;
pub mod llm;
pub mod memory;
pub mod persona;
pub mod prompt;
pub mod project;
pub mod tools;
pub mod git;
pub mod services;
pub mod state;

// Export commonly used items
pub use state::AppState;
pub use config::CONFIG;  // ADD THIS LINE - export global config for convenience
