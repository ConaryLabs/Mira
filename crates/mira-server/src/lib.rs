// crates/mira-server/src/lib.rs
// Mira - Memory and Intelligence Layer for AI Agents

#![allow(clippy::collapsible_if)]
#![allow(clippy::type_complexity)]
#![allow(clippy::too_many_arguments)]
#![warn(clippy::unwrap_used)]
#![warn(clippy::expect_used)]

pub mod background;
pub mod cartographer;
pub mod config;
pub mod context;
pub mod cross_project;
pub mod db;
pub mod embeddings;
pub mod error;
pub mod experts;
pub mod git;
pub mod hooks;
pub mod http;
pub mod identity;
pub mod indexer;
pub mod llm;
pub mod mcp;
pub mod mcp_client;
pub mod proactive;
pub mod project_files;
pub mod search;
pub mod tools;
pub mod utils;
pub use error::{MiraError, Result};
