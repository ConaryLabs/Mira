// src/lib.rs

pub mod api;
pub mod config;
pub mod llm;
pub mod memory; // <--- FIX #1
pub mod persona;
pub mod prompt;
pub mod project;
pub mod git;
pub mod services;
pub mod state;
pub mod utils;

// Export commonly used items
pub use state::AppState;
pub use config::CONFIG;
pub mod tools;
