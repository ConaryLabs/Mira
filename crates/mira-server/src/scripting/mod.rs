//! Rhai script execution engine for Mira's `run()` MCP tool.

mod bridge;
mod convert;
mod engine;
pub mod bindings;

pub use engine::execute_script;
