// backend/src/lib.rs

pub mod persona;
pub mod prompt;
pub mod llm;
pub mod memory;
pub mod handlers;
pub mod api;         // <-- Already present; keep!

pub mod tools;       // <-- Add this line to expose everything in src/tools/

// If you want to expose only mira_import, you could do:
// pub use tools::mira_import;

// If you want to keep tools private, remove this line and just use from main/bin targets.
