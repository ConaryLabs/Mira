// crates/mira-server/src/llm/openai_compat/mod.rs
// Shared OpenAI-compatible request/response handling for DeepSeek, GLM, etc.

mod request;
mod response;

pub use request::ChatRequest;
pub use response::{parse_chat_response, ChatResponse, ResponseChoice};
