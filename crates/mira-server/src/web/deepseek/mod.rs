// crates/mira-server/src/web/deepseek/mod.rs
// DeepSeek API client for Reasoner (V3.2) with tool calling support

mod client;
mod tools;
mod types;

// Re-export public API
pub use client::DeepSeekClient;
pub use tools::mira_tools;
pub use types::{ChatResult, FunctionCall, FunctionDef, Message, Tool, ToolCall, Usage};
