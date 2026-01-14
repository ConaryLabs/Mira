// crates/mira-server/src/cartographer/detection/mod.rs
// Polyglot module detection - unified interface for all supported languages

mod go;
mod node;
mod python;
pub mod rust;

use super::types::Module;
use std::path::Path;

/// Detect modules based on project type
///
/// This is the main entry point for module detection. It dispatches to the
/// appropriate language-specific detector based on the project_type.
pub fn detect_modules(project_path: &Path, project_type: &str) -> Vec<Module> {
    match project_type {
        "rust" => rust::detect(project_path),
        "python" => python::detect(project_path),
        "node" => node::detect(project_path),
        "go" => go::detect(project_path),
        _ => {
            tracing::warn!(
                "Unknown project type '{}', no modules detected",
                project_type
            );
            Vec::new()
        }
    }
}

/// Find entry points based on project type
pub fn find_entry_points(project_path: &Path, project_type: &str) -> Vec<String> {
    match project_type {
        "rust" => rust::find_entry_points(project_path),
        "python" => python::find_entry_points(project_path),
        "node" => node::find_entry_points(project_path),
        "go" => go::find_entry_points(project_path),
        _ => Vec::new(),
    }
}

/// Count lines in a module based on project type
pub fn count_lines_in_module(project_path: &Path, module_path: &str, project_type: &str) -> u32 {
    match project_type {
        "rust" => rust::count_lines_in_module(project_path, module_path),
        "python" => python::count_lines_in_module(project_path, module_path),
        "node" => node::count_lines_in_module(project_path, module_path),
        "go" => go::count_lines_in_module(project_path, module_path),
        _ => 0,
    }
}

/// Resolve import to module ID based on project type
pub fn resolve_import_to_module(
    import: &str,
    module_ids: &[(String, String)],
    project_type: &str,
) -> Option<String> {
    match project_type {
        "rust" => rust::resolve_import_to_module(import, module_ids),
        "python" => python::resolve_import_to_module(import, module_ids),
        "node" => node::resolve_import_to_module(import, module_ids),
        "go" => go::resolve_import_to_module(import, module_ids),
        _ => None,
    }
}

// Re-export for backward compatibility
pub use rust::detect as detect_rust_modules;
pub use rust::is_workspace;
pub use rust::parse_crate_name;
