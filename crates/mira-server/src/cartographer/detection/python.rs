// crates/mira-server/src/cartographer/detection/python.rs
// Python package/module detection from project structure

use super::super::types::Module;
use crate::project_files::walker::FileWalker;
use crate::utils::{path_to_string, relative_to};
use std::collections::HashSet;
use std::path::Path;

/// Detect Python modules from project structure
pub fn detect(project_path: &Path) -> Vec<Module> {
    let mut modules = Vec::new();

    tracing::info!("detect_python_modules: scanning {:?}", project_path);

    // Find the main package - look for pyproject.toml, setup.py, or top-level __init__.py
    let project_name = find_project_name(project_path);

    // Walk directory looking for packages (__init__.py) and modules (.py files)
    let mut seen_dirs: HashSet<String> = HashSet::new();

    for entry in FileWalker::new(project_path)
        .for_language("python")
        .max_depth(8)
        .walk_entries()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        let relative = relative_to(path, project_path);

        // Directory with __init__.py is a package
        if path.is_dir() {
            let init_py = path.join("__init__.py");
            if init_py.exists() {
                let module_path = path_to_string(relative);
                if !seen_dirs.contains(&module_path) && !module_path.is_empty() {
                    seen_dirs.insert(module_path.clone());

                    let module_name = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("unknown");

                    // Create module ID using dot notation like Python imports
                    let module_id = module_path.replace('/', ".");

                    modules.push(Module {
                        id: module_id,
                        name: module_name.to_string(),
                        path: module_path,
                        purpose: None,
                        exports: vec![],
                        depends_on: vec![],
                        symbol_count: 0,
                        line_count: 0,
                        detected_patterns: None,
                    });
                }
            }
        }

        // Top-level .py files (not in packages) are also modules
        if path.is_file() && path.extension().is_some_and(|e| e == "py") {
            let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

            // Skip __init__.py, setup.py, conftest.py and test files at root
            if file_name == "__init__.py"
                || file_name == "setup.py"
                || file_name == "conftest.py"
                || file_name.starts_with("test_")
            {
                continue;
            }

            // Only include top-level .py files (directly under project root or src/)
            let parent = path.parent();
            let is_top_level =
                parent == Some(project_path) || parent == Some(&project_path.join("src"));

            if is_top_level {
                let module_path = path_to_string(relative);
                if !seen_dirs.contains(&module_path) {
                    seen_dirs.insert(module_path.clone());

                    let module_name = file_name.trim_end_matches(".py");
                    let module_id = module_name.to_string();

                    modules.push(Module {
                        id: module_id,
                        name: module_name.to_string(),
                        path: module_path,
                        purpose: None,
                        exports: vec![],
                        depends_on: vec![],
                        symbol_count: 0,
                        line_count: 0,
                        detected_patterns: None,
                    });
                }
            }
        }
    }

    // If no modules found but we have a src/ layout, treat src/ contents as modules
    if modules.is_empty() {
        let src_dir = project_path.join("src");
        if src_dir.exists() {
            detect_in_src(
                &src_dir,
                project_path,
                &project_name,
                &mut modules,
                &mut seen_dirs,
            );
        }
    }

    tracing::info!("detect_python_modules: found {} modules", modules.len());
    for m in &modules {
        tracing::debug!("  module: {} at {}", m.id, m.path);
    }

    modules
}

fn detect_in_src(
    src_dir: &Path,
    project_path: &Path,
    _project_name: &str,
    modules: &mut Vec<Module>,
    seen_dirs: &mut HashSet<String>,
) {
    for entry in FileWalker::new(src_dir)
        .for_language("python")
        .max_depth(8)
        .walk_entries()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        let relative = relative_to(path, project_path);

        if path.is_dir() {
            let init_py = path.join("__init__.py");
            if init_py.exists() {
                let module_path = path_to_string(relative);
                if !seen_dirs.contains(&module_path) {
                    seen_dirs.insert(module_path.clone());

                    let module_name = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("unknown");

                    let module_id = module_path.replace("src/", "").replace('/', ".");

                    modules.push(Module {
                        id: module_id,
                        name: module_name.to_string(),
                        path: module_path,
                        purpose: None,
                        exports: vec![],
                        depends_on: vec![],
                        symbol_count: 0,
                        line_count: 0,
                        detected_patterns: None,
                    });
                }
            }
        }
    }
}

fn find_project_name(project_path: &Path) -> String {
    // Try pyproject.toml first
    let pyproject = project_path.join("pyproject.toml");
    if pyproject.exists() {
        if let Ok(content) = std::fs::read_to_string(&pyproject) {
            if let Some(name) = parse_pyproject_name(&content) {
                return name;
            }
        }
    }

    // Try setup.py
    let setup_py = project_path.join("setup.py");
    if setup_py.exists() {
        if let Ok(content) = std::fs::read_to_string(&setup_py) {
            if let Some(name) = parse_setup_py_name(&content) {
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

fn parse_pyproject_name(content: &str) -> Option<String> {
    let mut in_project = false;

    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_project = line == "[project]" || line == "[tool.poetry]";
        } else if in_project && line.starts_with("name") {
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

fn parse_setup_py_name(content: &str) -> Option<String> {
    // Simple regex-free extraction: look for name="..." or name='...'
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with("name") && line.contains('=') {
            if let Some(start) = line.find('"').or_else(|| line.find('\'')) {
                // Safe because we found an ASCII quote character at byte position `start`
                let quote = line.as_bytes()[start] as char;
                let rest = &line[start + 1..];
                if let Some(end) = rest.find(quote) {
                    return Some(rest[..end].to_string());
                }
            }
        }
    }
    None
}

/// Find Python entry points
pub fn find_entry_points(project_path: &Path) -> Vec<String> {
    let mut entries = Vec::new();

    // Common Python entry points
    let candidates = [
        "main.py",
        "__main__.py",
        "app.py",
        "run.py",
        "cli.py",
        "manage.py",
    ];

    for entry in FileWalker::new(project_path)
        .for_language("python")
        .max_depth(8)
        .walk_entries()
        .filter_map(|e| e.ok())
    {
        let name = entry
            .path()
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        if candidates.contains(&name) {
            if let Ok(rel) = entry.path().strip_prefix(project_path) {
                entries.push(path_to_string(rel));
            }
        }
    }

    entries.sort();
    entries
}

/// Resolve Python import to module ID
pub fn resolve_import_to_module(import: &str, module_ids: &[(String, String)]) -> Option<String> {
    // Python imports use dot notation: "package.subpackage.module"
    // Module IDs also use dot notation

    for (id, name) in module_ids {
        if id == import || import.starts_with(&format!("{}.", id)) || id.starts_with(import) {
            return Some(id.clone());
        }
        // Also check by name for relative imports
        if name == import {
            return Some(id.clone());
        }
    }
    None
}

/// Count lines in Python module
pub fn count_lines_in_module(project_path: &Path, module_path: &str) -> u32 {
    let full_path = project_path.join(module_path);

    let mut count = 0u32;

    if full_path.is_file() {
        if let Ok(content) = std::fs::read_to_string(&full_path) {
            return content.lines().count() as u32;
        }
    }

    for path in FileWalker::new(&full_path)
        .for_language("python")
        .walk_paths()
        .filter_map(|p| p.ok())
    {
        if let Ok(content) = std::fs::read_to_string(&path) {
            count += content.lines().count() as u32;
        }
    }
    count
}
