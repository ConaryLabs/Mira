// backend/src/cli/project/mod.rs
// Project detection and context module for CLI

pub mod context;
pub mod detector;

pub use context::{build_context_header, build_metadata, format_mira_md};
pub use detector::{DetectedProject, ProjectDetector};
