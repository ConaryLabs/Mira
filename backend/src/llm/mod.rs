// src/llm/mod.rs

pub mod embeddings;
pub mod provider;
pub mod reasoning_config;
pub mod structured;
pub mod types;

pub use reasoning_config::ReasoningConfig;

// OpenAI embeddings are now accessed via provider::OpenAiEmbeddings
