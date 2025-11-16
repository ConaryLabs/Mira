// src/project/mod.rs
pub mod store;
pub mod types;

// Re-export for easy use elsewhere
pub use types::{Artifact, ArtifactType, Project};
