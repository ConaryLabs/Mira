// crates/mira-types/src/lib.rs

//! Shared data contracts between the Mira native server and its clients.
//!
//! This crate provides the core domain model for:
//! - **Project context**: Mapping filesystem paths to database entities
//!
//! These types are designed to work across native and WASM builds,
//! with no native-only dependencies allowed.

use serde::{Deserialize, Serialize};

// ===================================================
// DOMAIN TYPES
// ===================================================

/// Represents the connection between a local filesystem path and a Mira database entity.
///
/// This context is required for almost all operations (indexing, code search, goals).
/// It ensures that data is scoped to the correct workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectContext {
    /// The persistent database ID for this project. Use this for all foreign keys.
    pub id: i64,
    /// The absolute filesystem path to the project root. Used for file operations.
    pub path: String,
    /// Human-readable display name (e.g., directory name or parsed from package.json/Cargo.toml).
    pub name: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================================
    // ProjectContext tests
    // ============================================================================

    #[test]
    fn test_project_context_serialize() {
        let ctx = ProjectContext {
            id: 1,
            path: "/home/user/project".to_string(),
            name: Some("my-project".to_string()),
        };
        let json = serde_json::to_string(&ctx).unwrap();
        assert!(json.contains("\"id\":1"));
        assert!(json.contains("/home/user/project"));
        assert!(json.contains("my-project"));
    }

    #[test]
    fn test_project_context_deserialize() {
        let json = r#"{"id": 42, "path": "/test/path", "name": "test"}"#;
        let ctx: ProjectContext = serde_json::from_str(json).unwrap();
        assert_eq!(ctx.id, 42);
        assert_eq!(ctx.path, "/test/path");
        assert_eq!(ctx.name, Some("test".to_string()));
    }

    #[test]
    fn test_project_context_name_optional() {
        let json = r#"{"id": 1, "path": "/test"}"#;
        let ctx: ProjectContext = serde_json::from_str(json).unwrap();
        assert_eq!(ctx.name, None);
    }
}
