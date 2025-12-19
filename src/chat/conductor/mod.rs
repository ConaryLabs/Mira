//! Conductor module - validation and JSON repair
//!
//! Provides tool validation and JSON auto-repair for DeepSeek reliability.

pub mod validation;

pub use validation::{repair_json, ToolSchemas};
