// src/llm/mod.rs

pub mod classification;
pub mod embeddings;
pub mod intent;
pub mod moderation;
pub mod provider;
pub mod responses;
pub mod router;  // NEW: Smart routing between DeepSeek/GPT-5
pub mod schema;
pub mod structured;
pub mod types;

// OpenAI embeddings are now accessed via provider::OpenAiEmbeddings
