// crates/mira-server/src/cartographer/detection/go.rs
// Go package detection from project structure

use super::super::types::Module;
use std::collections::HashSet;
use std::path::Path;
use crate::project_files::walker::FileWalker;

/// Detect Go packages from project structure
pub fn detect(project_path: &Path) -> Vec<Module> {
    let mut modules = Vec::new();

    tracing::info!("detect_go_modules: scanning {:?}", project_path);

    let module_name = find_module_name(project_path);

    // Walk directory looking for Go packages (directories with .go files)
    let mut seen_dirs: HashSet<String> = HashSet::new();

    for entry in FileWalker::new(project_path)
        .for_language("go")
        .max_depth(8)
        .walk_entries()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();

        // Only process directories
        if !path.is_dir() {
            continue;
        }

        // Check if this directory contains .go files (making it a package)
        let has_go_files = std::fs::read_dir(path)
            .map(|entries| {
                entries.filter_map(|e| e.ok()).any(|e| {
                    let name = e.file_name();
                    let name = name.to_string_lossy();
                    name.ends_with(".go") && !name.ends_with("_test.go")
                })
            })
            .unwrap_or(false);

        if !has_go_files {
            continue;
        }

        let relative = path.strip_prefix(project_path).unwrap_or(path);
        let module_path = relative.to_string_lossy().to_string();

        if seen_dirs.contains(&module_path) {
            continue;
        }
        seen_dirs.insert(module_path.clone());

        let package_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("main");

        // Create module ID: module_name/relative_path or just package name for root
        let module_id = if module_path.is_empty() {
            module_name.clone()
        } else {
            format!("{}/{}", module_name, module_path)
        };

        modules.push(Module {
            id: module_id,
            name: package_name.to_string(),
            path: if module_path.is_empty() {
                ".".to_string()
            } else {
                module_path
            },
            purpose: None,
            exports: vec![],
            depends_on: vec![],
            symbol_count: 0,
            line_count: 0,
        });
    }

    tracing::info!("detect_go_modules: found {} modules", modules.len());
    for m in &modules {
        tracing::debug!("  module: {} at {}", m.id, m.path);
    }

    modules
}

fn find_module_name(project_path: &Path) -> String {
    // Parse go.mod for module name
    let go_mod = project_path.join("go.mod");
    if go_mod.exists() {
        if let Ok(content) = std::fs::read_to_string(&go_mod) {
            if let Some(name) = parse_go_mod_name(&content) {
                return name;
            }
        }
    }

    // Fall back to directory name
    project_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project")
        .to_string()
}

fn parse_go_mod_name(content: &str) -> Option<String> {
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with("module ") {
            let name = line.strip_prefix("module ")?.trim();
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }
    None
}

/// Find Go entry points
pub fn find_entry_points(project_path: &Path) -> Vec<String> {
    let mut entries = Vec::new();

    // Look for main packages (directories with main.go or package main declaration)
    for entry in FileWalker::new(project_path)
        .for_language("go")
        .max_depth(8)
        .walk_entries()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();

        // Look for main.go or cmd/ directories
        if path.is_file() {
            let name = entry.path().file_name().unwrap_or_default().to_string_lossy();
            if name == "main.go" {
                if let Ok(rel) = path.strip_prefix(project_path) {
                    entries.push(rel.to_string_lossy().to_string());
                }
            }
        }
    }

    // Also check cmd/ subdirectories
    let cmd_dir = project_path.join("cmd");
    if cmd_dir.exists() {
        if let Ok(cmd_entries) = std::fs::read_dir(&cmd_dir) {
            for entry in cmd_entries.filter_map(|e| e.ok()) {
                let path = entry.path();
                if path.is_dir() {
                    let main_go = path.join("main.go");
                    if main_go.exists() {
                        if let Ok(rel) = main_go.strip_prefix(project_path) {
                            entries.push(rel.to_string_lossy().to_string());
                        }
                    }
                }
            }
        }
    }

    entries.sort();
    entries.dedup();
    entries
}

/// Resolve Go import to module ID
pub fn resolve_import_to_module(import: &str, module_ids: &[(String, String)]) -> Option<String> {
    // Go imports are full paths: "github.com/user/repo/pkg/subpkg"
    for (id, name) in module_ids {
        if id == import || import.ends_with(id) || id.ends_with(import) {
            return Some(id.clone());
        }
        // Also check by package name
        if name == import {
            return Some(id.clone());
        }
    }
    None
}

/// Count lines in Go package
pub fn count_lines_in_module(project_path: &Path, module_path: &str) -> u32 {
    let full_path = if module_path == "." {
        project_path.to_path_buf()
    } else {
        project_path.join(module_path)
    };

    let mut count = 0u32;

    // For Go, we only count .go files directly in the package (no recursion)
    if let Ok(entries) = std::fs::read_dir(&full_path) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "go") {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    count += content.lines().count() as u32;
                }
            }
        }
    }

    count
}
