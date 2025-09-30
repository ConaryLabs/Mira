// src/llm/structured/mod.rs

pub mod types;
pub mod processor;
pub mod validator;
pub mod code_fix_processor;
pub mod claude_processor;  // NEW: Declare as sibling module

pub use types::*;
pub use processor::*;
pub use validator::*;
pub use code_fix_processor::*;
