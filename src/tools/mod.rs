// src/tools/mod.rs
// MCP Tool modules - organized by domain for Claude Code augmentation

pub mod analytics;
pub mod build_intel;
pub mod code_intel;
pub mod documents;
pub mod git_intel;
pub mod memory;
pub mod project;
pub mod response;
pub mod semantic;
pub mod sessions;
pub mod tasks;
pub mod workspace;
pub mod types;

// Re-export for use in main.rs
pub use types::*;
pub use semantic::SemanticSearch;
pub use response::{to_mcp_err, json_response, text_response, vec_response, option_response};
