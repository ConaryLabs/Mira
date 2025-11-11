// src/llm/mod.rs

pub mod embeddings;
pub mod provider;
pub mod structured;
pub mod types;
pub mod reasoning_config;

pub use reasoning_config::ReasoningConfig;

// OpenAI embeddings are now accessed via provider::OpenAiEmbeddings
