// crates/mira-server/src/llm/mod.rs
// LLM inference clients (DeepSeek)

mod context_budget;
mod deepseek;
mod factory;
mod http_client;
pub mod logging;
pub mod openai_compat;
pub mod pricing;
mod prompt;
mod provider;
pub mod sampling;
mod types;
mod ollama;
mod zhipu;

pub use context_budget::{
    CONTEXT_BUDGET, estimate_message_tokens, estimate_tokens, truncate_messages_to_budget,
    truncate_messages_to_default_budget,
};
pub use deepseek::DeepSeekClient;
pub use factory::ProviderFactory;
pub use http_client::LlmHttpClient;
pub use pricing::{ModelPricing, chat_with_usage, get_pricing, record_llm_usage};
pub use prompt::{EXPERT_CODE_TOOLS, EXPERT_WEB_TOOLS, PromptBuilder};
pub use provider::{LlmClient, NormalizedUsage, Provider};
pub use types::{ChatResult, FunctionCall, FunctionDef, Message, Tool, ToolCall, Usage};
pub use ollama::OllamaClient;
pub use zhipu::ZhipuClient;
