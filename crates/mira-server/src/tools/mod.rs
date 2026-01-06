//! crates/mira-server/src/tools/mod.rs
//! Unified tool core for Mira
//!
//! Provides a single implementation of all tools that can be used by both
//! the web chat interface (/api/chat) and the MCP server.

pub mod core;
pub mod web;
pub mod mcp;

/// Re-export core tools for convenience
pub use core::*;
