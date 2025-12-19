//! Chat module - DeepSeek-powered coding assistant
//!
//! This module provides the chat functionality for Mira Studio:
//! - DeepSeek V3.2 as the primary model
//! - Council tool for consulting GPT 5.2, Opus 4.5, Gemini 3 Pro
//! - Full Mira context injection
//! - Persistent memory and session management

pub mod conductor;
pub mod config;
pub mod context;
pub mod provider;
pub mod reasoning;
pub mod server;
pub mod session;
pub mod tools;

/// Memory collection name (alias for backwards compatibility)
pub const COLLECTION_MEMORY: &str = mira_core::COLLECTION_CONVERSATION;

// Re-export key types for external use
pub use server::{create_router, AppState};
