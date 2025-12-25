//! Mira power armor operations - task, goal, correction, decision, rejected_approach
//!
//! This module re-exports from domain-specific submodules for backwards compatibility.
//! New code should import directly from the submodules.

// Re-export all types and functions from submodules
pub use super::tasks::*;
pub use super::goals::*;
pub use super::corrections::*;
pub use super::decisions::*;
pub use super::rejected::*;

// Shared helper used by corrections and rejected modules
pub(crate) fn normalize_json_array(input: &Option<String>) -> Option<String> {
    input.as_ref().map(|s| {
        if s.trim().starts_with('[') {
            s.clone()
        } else {
            let items: Vec<&str> = s
                .split(',')
                .map(|x| x.trim())
                .filter(|x| !x.is_empty())
                .collect();
            serde_json::to_string(&items).unwrap_or_else(|_| "[]".to_string())
        }
    })
}
