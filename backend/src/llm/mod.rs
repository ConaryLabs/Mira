// src/llm/mod.rs

pub mod embeddings;
pub mod provider;
pub mod router;
pub mod types;

pub use router::{DeepSeekModel, ModelRouter, TaskAnalysis};

// OpenAI embeddings are now accessed via provider::OpenAiEmbeddings
