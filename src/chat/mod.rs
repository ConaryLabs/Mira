//! Chat module - DeepSeek-powered coding assistant
//!
//! This module provides the chat functionality for Mira Studio:
//! - DeepSeek V3.2 as the primary model
//! - Council tool for consulting GPT 5.2, Opus 4.5, Gemini 3 Pro
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
