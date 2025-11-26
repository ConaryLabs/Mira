// src/project/mod.rs
pub mod guidelines;
pub mod store;
pub mod types;

// Re-export for easy use elsewhere
pub use guidelines::{ProjectGuidelines, ProjectGuidelinesService};
pub use types::{Artifact, ArtifactType, Project};
