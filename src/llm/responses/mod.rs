// src/llm/responses/mod.rs
// Image generation only - chat uses Claude in src/llm/structured/

pub mod image;

pub use image::{ImageGenerationManager, ImageOptions, ImageGenerationResponse, ImageData};
