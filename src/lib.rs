// src/lib.rs

pub mod api;
pub mod handlers;
pub mod llm;
pub mod memory;
pub mod persona;
pub mod prompt;
pub mod project;
pub mod tools;
pub mod git;
pub mod services;
pub mod state;     // Add the new state module
// Removed: pub mod session (empty directory)
// Removed: pub mod db (empty directory)
// Removed: pub mod context (doesn't exist)

// Optional: export commonly used items
pub use state::AppState;  // Changed from handlers::AppState
