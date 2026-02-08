// crates/mira-server/src/llm/mod.rs
// LLM inference clients (DeepSeek)

mod circuit_breaker;
mod context_budget;
mod deepseek;
mod factory;
mod http_client;
pub mod logging;
mod ollama;
pub mod openai_compat;
pub mod pricing;
mod prompt;
mod provider;
pub mod sampling;
mod types;
mod zhipu;

pub use circuit_breaker::CircuitBreaker;
pub use context_budget::{
    CONTEXT_BUDGET, estimate_message_tokens, estimate_tokens, truncate_messages_to_budget,
    truncate_messages_to_default_budget,
};
pub use deepseek::DeepSeekClient;
pub use factory::ProviderFactory;
pub use http_client::LlmHttpClient;
pub use ollama::OllamaClient;
pub use pricing::{ModelPricing, chat_with_usage, get_pricing, record_llm_usage};
pub use prompt::PromptBuilder;
pub use provider::{LlmClient, NormalizedUsage, Provider};
pub use types::{ChatResult, FunctionCall, FunctionDef, Message, Tool, ToolCall, Usage};
pub use zhipu::ZhipuClient;
