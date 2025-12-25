//! Tool loop implementations for each advisory provider
//!
//! Each module handles the multi-turn tool calling loop for a specific LLM.

pub mod gpt;
pub mod gemini;
pub mod deepseek;
pub mod opus;

pub use gpt::ask_with_tools_gpt;
pub use gemini::ask_with_tools_gemini;
pub use deepseek::ask_with_tools_deepseek;
pub use opus::ask_with_tools_opus;
