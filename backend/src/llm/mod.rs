// src/llm/mod.rs

pub mod embeddings;
pub mod provider;
pub mod router;
pub mod types;

pub use router::{ReasoningRouter, TaskAnalysis, Complexity, TaskType};

// OpenAI embeddings are now accessed via provider::OpenAiEmbeddings
