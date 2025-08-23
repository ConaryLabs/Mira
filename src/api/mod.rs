// src/api/mod.rs
// API module with clean, organized structure

pub mod ws;
pub mod http;
pub mod types;
pub mod error;

// Re-export commonly used items for external convenience
pub use error::{ApiError, ApiResult};
pub use types::*;

// Note: Router composition is handled directly in main.rs
// No additional router aggregation needed here
