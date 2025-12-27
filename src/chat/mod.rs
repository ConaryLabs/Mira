//! Chat module - Gemini-powered coding assistant
//!
//! This module provides the chat functionality for Mira Studio:
//! - Gemini 3 Flash/Pro as the primary model
//! - Full Mira context injection
//! - Persistent memory and session management

#![allow(dead_code)] // Some items are infrastructure for future use

pub mod conductor;
pub mod context;
pub mod provider;
pub mod server;
pub mod session;
pub mod tools;

// Re-export key types for external use
pub use server::{create_router, AppState};
