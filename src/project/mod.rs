// src/project/mod.rs
pub mod types;
pub mod store;

// Re-export for easy use elsewhere
pub use types::{Project, Artifact, ArtifactType};
