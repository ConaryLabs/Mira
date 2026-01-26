// crates/mira-server/src/lib.rs
// Mira - Memory and Intelligence Layer for AI Agents
// Test edit for file watcher verification

#![allow(clippy::collapsible_if)]
#![allow(clippy::type_complexity)]
#![allow(clippy::too_many_arguments)]
#![warn(clippy::unwrap_used)]
#![warn(clippy::expect_used)]

pub mod background;
pub mod cartographer;
pub mod config;
pub mod project_files;
pub mod db;
pub mod embeddings;
pub mod hooks;
pub mod identity;
pub mod indexer;
pub mod llm;
pub mod mcp;
pub mod search;
pub mod context;
pub mod tools;
pub mod http;
pub mod error;
pub mod proactive;
pub mod experts;
pub mod cross_project;
pub mod git;
pub mod utils;
pub use error::{MiraError, Result};
