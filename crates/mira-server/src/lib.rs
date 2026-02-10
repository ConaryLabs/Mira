// crates/mira-server/src/lib.rs
// Mira - Memory and Intelligence Layer for AI Agents

#![warn(clippy::unwrap_used)]
#![warn(clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod background;
pub mod cartographer;
pub mod config;
pub mod context;
pub mod db;
pub mod embeddings;
pub mod entities;
pub mod error;
pub mod fuzzy;
pub mod git;
pub mod hooks;
pub mod http;
pub mod identity;
pub mod indexer;
pub mod llm;
pub mod mcp;
pub mod proactive;
pub mod project_files;
pub mod search;
pub mod tasks;
pub mod tools;
pub mod utils;
pub use error::{MiraError, Result};
