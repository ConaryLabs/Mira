// src/api/mod.rs
// CLEANED: Removed unused api_router() function and simplified structure

pub mod ws;
pub mod http;
pub mod types;
pub mod error;

// Re-export commonly used items for external convenience
pub use error::{ApiError, ApiResult};
pub use types::*;

// Router composition is handled directly in main.rs
