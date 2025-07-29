// src/lib.rs

pub mod api;
pub mod handlers;
pub mod llm;
pub mod memory;
pub mod persona;
pub mod prompt;
pub mod session;
pub mod project;
pub mod tools;
pub mod git;
pub mod services; // <--- add this line!

// Optional: export commonly used items
pub use handlers::AppState;
