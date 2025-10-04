// src/llm/mod.rs

pub mod classification;
pub mod client;
pub mod embeddings;
pub mod intent;
pub mod moderation;
pub mod provider;  // NEW: Multi-provider support
pub mod responses;
pub mod schema;
pub mod structured;
pub mod types;

pub use client::OpenAIClient;
