// crates/mira-server/src/search/utils.rs
// Shared utilities for search operations

use mira_types::ProjectContext;

/// Convert embedding vector to bytes for sqlite-vec queries
pub fn embedding_to_bytes(embedding: &[f32]) -> Vec<u8> {
    embedding.iter().flat_map(|f| f.to_le_bytes()).collect()
}

/// Format project context header for tool responses
pub fn format_project_header(project: Option<&ProjectContext>) -> String {
    match project {
        Some(p) => format!(
            "[Project: {} @ {}]\n\n",
            p.name.as_deref().unwrap_or("Unknown"),
            p.path
        ),
        None => String::new(),
    }
}

/// Convert distance to similarity score (0.0 to 1.0)
pub fn distance_to_score(distance: f32) -> f32 {
    1.0 - distance.clamp(0.0, 1.0)
}
