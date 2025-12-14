// src/lib.rs
// Mira Power Suit - MCP Server for Claude Code

#![allow(clippy::collapsible_if)] // Nested ifs often clearer than let-chains
#![allow(clippy::field_reassign_with_default)] // IndexStats built incrementally
#![allow(clippy::type_complexity)] // Query result tuples are inherently complex
#![allow(clippy::too_many_arguments)] // Parser walk functions need context

pub mod tools;
pub mod indexer;
pub mod server;
