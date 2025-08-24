// src/llm/responses/mod.rs
// PHASE 3 UPDATE: Added image module export and ImageGenerationManager

pub mod manager;
pub mod vector_store;
pub mod thread;
pub mod types;
pub mod image; // PHASE 3 NEW: Image generation module

pub use manager::ResponsesManager;
pub use vector_store::VectorStoreManager;
pub use thread::ThreadManager;
pub use types::*;

// PHASE 3 NEW: Export image generation functionality
pub use image::{
    ImageGenerationManager, 
    ImageOptions, 
    ImageGenerationResponse, 
    GeneratedImage
};
