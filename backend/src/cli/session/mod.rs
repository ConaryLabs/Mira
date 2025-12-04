// backend/src/cli/session/mod.rs
// Session management module for CLI

pub mod picker;
pub mod types;

// Note: store.rs is deprecated - CLI now uses WebSocket APIs via MiraClient
#[allow(dead_code)]
pub mod store;

pub use picker::{simple_session_list, SessionPicker};
pub use types::{CliSession, SessionFilter};
