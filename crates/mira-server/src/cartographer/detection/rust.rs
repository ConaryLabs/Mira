// crates/mira-server/src/cartographer/detection/rust.rs
// Rust module detection from project structure

use super::super::types::Module;
use std::collections::HashSet;
use std::path::Path;
use crate::project_files::walker::FileWalker;

/// Detect Rust modules from project structure
pub fn detect(project_path: &Path) -> Vec<Module> {
    let mut modules = Vec::new();

    tracing::info!("detect_rust_modules: scanning {:?}", project_path);

    // Find all Cargo.toml files (workspace members)
    let cargo_tomls: Vec<_> = FileWalker::new(project_path)
        .max_depth(8)
        .walk_paths()
        .filter_map(|p| p.ok())
        .filter(|p| p.file_name().map(|n| n == "Cargo.toml").unwrap_or(false))
        .filter(|p| p != &project_path.join("Cargo.toml") || !is_workspace(p))
        .collect();

    tracing::info!("Found {} Cargo.toml files", cargo_tomls.len());

    for entry in cargo_tomls {
        let crate_root = entry.parent().unwrap_or(project_path);
        let crate_name = parse_crate_name(&entry).unwrap_or_else(|| {
            crate_root
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string()
        });

        let src_dir = crate_root.join("src");
        if !src_dir.exists() {
            continue;
        }

        // Walk src directory looking for modules
        detect_modules_in_src(&src_dir, &crate_name, project_path, &mut modules);
    }

    // If no crates found, try the project root directly
    if modules.is_empty() {
        let src_dir = project_path.join("src");
        if src_dir.exists() {
            let crate_name = project_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("project")
                .to_string();
            detect_modules_in_src(&src_dir, &crate_name, project_path, &mut modules);
        }
    }

    tracing::info!("detect_rust_modules: found {} modules", modules.len());
    for m in &modules {
        tracing::debug!("  module: {} at {}", m.id, m.path);
    }

    modules
}

pub fn is_workspace(cargo_toml: &Path) -> bool {
    std::fs::read_to_string(cargo_toml)
        .map(|c| c.contains("[workspace]"))
        .unwrap_or(false)
}

pub fn parse_crate_name(cargo_toml: &Path) -> Option<String> {
    let content = std::fs::read_to_string(cargo_toml).ok()?;
    let mut in_package = false;

    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_package = line == "[package]";
        } else if in_package && line.starts_with("name") {
            if let Some(name) = line.split('=').nth(1) {
                let name = name.trim().trim_matches('"').trim_matches('\'');
                if !name.is_empty() {
                    return Some(name.to_string());
                }
            }
        }
    }
    None
}

fn detect_modules_in_src(
    src_dir: &Path,
    crate_name: &str,
    project_path: &Path,
    modules: &mut Vec<Module>,
) {
    let mut seen_dirs: HashSet<String> = HashSet::new();

    for path in FileWalker::new(src_dir)
        .for_language("rust")
        .walk_paths()
        .filter_map(|p| p.ok())
    {
        let relative = path.strip_prefix(project_path).unwrap_or(&path);
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        // Entry points become top-level modules
        if file_name == "lib.rs" || file_name == "main.rs" {
            let module_path = relative
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();

            if !seen_dirs.contains(&module_path) {
                seen_dirs.insert(module_path.clone());
                modules.push(Module {
                    id: crate_name.to_string(),
                    name: crate_name.to_string(),
                    path: module_path,
                    purpose: None,
                    exports: vec![],
                    depends_on: vec![],
                    symbol_count: 0,
                    line_count: 0,
                });
            }
        }
        // mod.rs indicates a module directory
        else if file_name == "mod.rs" {
            if let Some(parent) = path.parent() {
                let module_name = parent
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown");
                let module_path = parent
                    .strip_prefix(project_path)
                    .unwrap_or(parent)
                    .to_string_lossy()
                    .to_string();

                // Create module ID: crate_name/relative_module_path
                let src_relative = parent.strip_prefix(src_dir).unwrap_or(parent);
                let module_id = if src_relative.as_os_str().is_empty() {
                    crate_name.to_string()
                } else {
                    format!("{}/{}", crate_name, src_relative.to_string_lossy())
                };

                if !seen_dirs.contains(&module_path) {
                    seen_dirs.insert(module_path.clone());
                    modules.push(Module {
                        id: module_id,
                        name: module_name.to_string(),
                        path: module_path,
                        purpose: None,
                        exports: vec![],
                        depends_on: vec![],
                        symbol_count: 0,
                        line_count: 0,
                    });
                }
            }
        }
        // Regular .rs files in src/ are also modules
        else if path.parent() == Some(src_dir) && file_name != "lib.rs" && file_name != "main.rs"
        {
            let module_name = file_name.trim_end_matches(".rs");
            let module_id = format!("{}/{}", crate_name, module_name);
            let module_path = relative.to_string_lossy().to_string();

            if !seen_dirs.contains(&module_path) {
                seen_dirs.insert(module_path.clone());
                modules.push(Module {
                    id: module_id,
                    name: module_name.to_string(),
                    path: module_path,
                    purpose: None,
                    exports: vec![],
                    depends_on: vec![],
                    symbol_count: 0,
                    line_count: 0,
                });
            }
        }
    }
}

/// Find Rust entry points
pub fn find_entry_points(project_path: &Path) -> Vec<String> {
    let mut entries = Vec::new();

    for path in FileWalker::new(project_path)
        .for_language("rust")
        .max_depth(8)
        .walk_paths()
        .filter_map(|p| p.ok())
    {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name == "main.rs" || name == "lib.rs" {
            if let Ok(rel) = path.strip_prefix(project_path) {
                entries.push(rel.to_string_lossy().to_string());
            }
        }
    }

    entries.sort();
    entries
}

/// Resolve Rust import to module ID
pub fn resolve_import_to_module(import: &str, module_ids: &[(String, String)]) -> Option<String> {
    // Convert "crate::foo::bar" to check against module IDs
    let import = import
        .replace("crate::", "")
        .replace("super::", "")
        .replace("::", "/");

    // Find matching module
    for (id, name) in module_ids {
        if id.ends_with(&import) || import.starts_with(name) {
            return Some(id.clone());
        }
    }
    None
}

/// Count lines in Rust module
pub fn count_lines_in_module(project_path: &Path, module_path: &str) -> u32 {
    let full_path = project_path.join(module_path);

    let mut count = 0u32;
    for path in FileWalker::new(&full_path)
        .for_language("rust")
        .walk_paths()
        .filter_map(|p| p.ok())
    {
        if let Ok(content) = std::fs::read_to_string(&path) {
            count += content.lines().count() as u32;
        }
    }
    count
}
