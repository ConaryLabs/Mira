// src/llm/structured/mod.rs
// New module for structured responses - replacing streaming SSE nightmare

pub mod types;
pub mod processor;
pub mod validator;

pub use types::*;
pub use processor::*;
pub use validator::*;
