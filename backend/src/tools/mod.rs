// src/tools/mod.rs
// MCP Tool modules - organized by domain for Claude Code augmentation

pub mod analytics;
pub mod build_intel;
pub mod code_intel;
pub mod documents;
pub mod git_intel;
pub mod memory;
pub mod project;
pub mod semantic;
pub mod sessions;
pub mod tasks;
pub mod workspace;
pub mod types;

// Re-export request types for use in main.rs
pub use types::*;
pub use semantic::SemanticSearch;
