// src/llm/mod.rs

pub mod embeddings;
pub mod provider;
pub mod reasoning_config;
pub mod router;
pub mod structured;
pub mod types;

pub use reasoning_config::ReasoningConfig;
pub use router::{DeepSeekModel, ModelRouter, TaskAnalysis};

// OpenAI embeddings are now accessed via provider::OpenAiEmbeddings
