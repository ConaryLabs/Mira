// crates/mira-server/src/llm/mod.rs
// LLM inference clients (DeepSeek, Gemini)

mod types;
mod deepseek;
mod gemini;
pub mod openai_compat;
mod provider;
mod factory;
mod prompt;
mod context_budget;
mod http_client;
pub mod pricing;

pub use types::{ChatResult, FunctionCall, FunctionDef, Message, Tool, ToolCall, Usage};
pub use deepseek::DeepSeekClient;
pub use gemini::GeminiClient;
pub use provider::{LlmClient, NormalizedUsage, Provider};
pub use factory::ProviderFactory;
pub use prompt::PromptBuilder;
pub use context_budget::{CONTEXT_BUDGET, estimate_tokens, estimate_message_tokens, truncate_messages_to_budget};
pub use http_client::LlmHttpClient;
pub use pricing::{get_pricing, record_llm_usage, ModelPricing};
