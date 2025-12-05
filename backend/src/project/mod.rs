// backend/src/project/mod.rs
pub mod guidelines;
pub mod store;
pub mod tasks;
pub mod types;

// Re-export for easy use elsewhere
pub use guidelines::{ProjectGuidelines, ProjectGuidelinesService};
pub use store::ProjectStore;
pub use tasks::{
    NewProjectTask, ProjectTask, ProjectTaskService, ProjectTaskStatus, TaskContext, TaskPriority,
    TaskSession,
};
pub use types::{Artifact, ArtifactType, Project};
