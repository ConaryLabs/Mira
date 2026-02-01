// crates/mira-server/src/llm/mod.rs
// LLM inference clients (DeepSeek, Gemini)

mod context_budget;
mod deepseek;
mod factory;
mod gemini;
mod http_client;
pub mod openai_compat;
pub mod pricing;
mod prompt;
mod provider;
pub mod sampling;
mod types;

pub use context_budget::{
    CONTEXT_BUDGET, estimate_message_tokens, estimate_tokens, truncate_messages_to_budget,
};
pub use factory::ProviderFactory;
pub use gemini::GeminiClient;
pub use http_client::LlmHttpClient;
pub use pricing::{ModelPricing, get_pricing, record_llm_usage};
pub use prompt::PromptBuilder;
pub use provider::{LlmClient, NormalizedUsage, Provider};
pub use types::{ChatResult, FunctionCall, FunctionDef, Message, Tool, ToolCall, Usage};
