// src/api/mod.rs
// WebSocket-only API module

pub mod ws;
pub mod types;
pub mod error;

// Re-export commonly used items
pub use error::{ApiError, ApiResult};
pub use types::*;
