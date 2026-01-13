// crates/mira-server/src/llm/deepseek/mod.rs
// DeepSeek API client for Reasoner (V3.2) with tool calling support

mod client;
mod types;

// Re-export public API
pub use client::DeepSeekClient;
pub use types::{ChatResult, FunctionCall, FunctionDef, Message, Tool, ToolCall, Usage};
