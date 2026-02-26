// crates/mira-server/src/cartographer/detection/mod.rs
// Polyglot module detection - unified interface for all supported languages

mod go;
mod node;
mod python;
pub mod rust;

use super::types::Module;
use crate::project_files::FileWalker;
use std::path::Path;

/// Count lines by walking files with `FileWalker` for a given language.
/// If `module_path` is a single file, counts its lines directly.
pub(crate) fn count_lines_with_walker(
    project_path: &Path,
    module_path: &str,
    language: &'static str,
) -> u32 {
    let full_path = project_path.join(module_path);

    // Single-file modules
    if full_path.is_file() {
        return std::fs::read_to_string(&full_path)
            .map(|c| c.lines().count() as u32)
            .unwrap_or(0);
    }

    let mut count = 0u32;
    for path in FileWalker::new(&full_path)
        .for_language(language)
        .walk_paths()
        .filter_map(|p| p.ok())
    {
        if let Ok(content) = std::fs::read_to_string(&path) {
            count += content.lines().count() as u32;
        }
    }
    count
}

/// Detect modules based on project type
///
/// This is the main entry point for module detection. It dispatches to the
/// appropriate language-specific detector based on the project_type.
pub fn detect_modules(project_path: &Path, project_type: &str) -> Vec<Module> {
    detect_modules_for_types(project_path, &[project_type])
}

/// Detect modules for multiple project types (polyglot support).
///
/// Merges module detection results from all supported languages present in
/// the project. Unsupported types (e.g., "java", "unknown") are skipped.
pub fn detect_modules_for_types(project_path: &Path, project_types: &[&str]) -> Vec<Module> {
    let mut modules = Vec::new();
    for &project_type in project_types {
        match project_type {
            "rust" => modules.extend(rust::detect(project_path)),
            "python" => modules.extend(python::detect(project_path)),
            "node" => modules.extend(node::detect(project_path)),
            "go" => modules.extend(go::detect(project_path)),
            "java" => {
                tracing::info!(
                    "Java project detected at '{}' but not yet supported for module detection",
                    project_path.display()
                );
            }
            "unknown" => {}
            _ => {
                tracing::warn!(
                    "Unknown project type '{}', no modules detected",
                    project_type
                );
            }
        }
    }
    modules
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
