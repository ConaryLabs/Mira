// src/llm/mod.rs
// LLM module exports and submodule declarations

pub mod classification;
pub mod client;
pub mod embeddings;
pub mod intent;
pub mod moderation;
pub mod responses;
pub mod schema;
pub mod structured;  // NEW: Structured response types and processing
pub mod types;

// Export the main client
pub use client::OpenAIClient;

// Export structured response types for other modules
pub use structured::{
    StructuredGPT5Response, MessageAnalysis, GPT5Metadata, CompleteResponse,
    validate_response, validate_complete_response,
    build_structured_request, extract_metadata, extract_structured_content
};
