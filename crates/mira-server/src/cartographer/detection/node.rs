// crates/mira-server/src/cartographer/detection/node.rs
// Node.js/TypeScript module detection from project structure

use super::super::types::Module;
use std::collections::HashSet;
use std::path::Path;
use walkdir::WalkDir;

/// Detect Node.js/TypeScript modules from project structure
pub fn detect(project_path: &Path) -> Vec<Module> {
    let mut modules = Vec::new();

    tracing::info!("detect_node_modules: scanning {:?}", project_path);

    let project_name = find_project_name(project_path);

    // Find workspace packages if this is a monorepo
    let workspaces = find_workspaces(project_path);

    if workspaces.is_empty() {
        // Single package project
        detect_in_package(project_path, &project_name, project_path, &mut modules);
    } else {
        // Monorepo - detect modules in each workspace
        for workspace_path in workspaces {
            let pkg_name = find_package_name(&workspace_path);
            detect_in_package(&workspace_path, &pkg_name, project_path, &mut modules);
        }
    }

    tracing::info!("detect_node_modules: found {} modules", modules.len());
    for m in &modules {
        tracing::debug!("  module: {} at {}", m.id, m.path);
    }

    modules
}

fn detect_in_package(
    package_path: &Path,
    package_name: &str,
    project_path: &Path,
    modules: &mut Vec<Module>,
) {
    let mut seen_dirs: HashSet<String> = HashSet::new();

    // Look for src/ directory first (common in TypeScript projects)
    let src_dir = package_path.join("src");
    let search_root = if src_dir.exists() {
        src_dir
    } else {
        package_path.to_path_buf()
    };

    for entry in WalkDir::new(&search_root)
        .max_depth(5)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            !name.starts_with('.')
                && name != "node_modules"
                && name != "dist"
                && name != "build"
                && name != "coverage"
                && name != "__tests__"
                && name != "__mocks__"
        })
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        let relative = path.strip_prefix(project_path).unwrap_or(path);

        // Directory with index.ts/js is a module
        if path.is_dir() {
            let has_index = has_index_file(path);
            if has_index {
                let module_path = relative.to_string_lossy().to_string();
                if !seen_dirs.contains(&module_path) && !module_path.is_empty() {
                    seen_dirs.insert(module_path.clone());

                    let module_name = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("unknown");

                    // Create module ID: package_name/relative_path
                    let pkg_relative = path.strip_prefix(package_path).unwrap_or(path);
                    let module_id = if pkg_relative.as_os_str().is_empty() {
                        package_name.to_string()
                    } else {
                        format!("{}/{}", package_name, pkg_relative.to_string_lossy())
                    };

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

        // Top-level .ts/.js files in src/ are also modules
        if path.is_file() && is_source_file(path) {
            let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

            // Skip index files (they represent the directory), test files, and config
            if file_name.starts_with("index.")
                || file_name.ends_with(".test.ts")
                || file_name.ends_with(".test.js")
                || file_name.ends_with(".spec.ts")
                || file_name.ends_with(".spec.js")
                || file_name.ends_with(".d.ts")
                || file_name.ends_with(".config.ts")
                || file_name.ends_with(".config.js")
            {
                continue;
            }

            // Only include files directly in src/
            let parent = path.parent();
            let is_direct_child = parent == Some(&search_root);

            if is_direct_child {
                let module_path = relative.to_string_lossy().to_string();
                if !seen_dirs.contains(&module_path) {
                    seen_dirs.insert(module_path.clone());

                    let module_name = file_name
                        .trim_end_matches(".ts")
                        .trim_end_matches(".tsx")
                        .trim_end_matches(".js")
                        .trim_end_matches(".jsx");

                    let module_id = format!("{}/{}", package_name, module_name);

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

    // If no modules found, add the package itself as a module
    if modules.is_empty() {
        let module_path = package_path
            .strip_prefix(project_path)
            .unwrap_or(package_path)
            .to_string_lossy()
            .to_string();

        modules.push(Module {
            id: package_name.to_string(),
            name: package_name.to_string(),
            path: module_path,
            purpose: None,
            exports: vec![],
            depends_on: vec![],
            symbol_count: 0,
            line_count: 0,
        });
    }
}

fn has_index_file(dir: &Path) -> bool {
    let index_names = ["index.ts", "index.tsx", "index.js", "index.jsx"];
    index_names.iter().any(|name| dir.join(name).exists())
}

fn is_source_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|ext| matches!(ext, "ts" | "tsx" | "js" | "jsx"))
        .unwrap_or(false)
}

fn find_project_name(project_path: &Path) -> String {
    find_package_name(project_path)
}

fn find_package_name(package_path: &Path) -> String {
    let package_json = package_path.join("package.json");
    if package_json.exists() {
        if let Ok(content) = std::fs::read_to_string(&package_json) {
            if let Some(name) = parse_package_json_name(&content) {
                return name;
            }
        }
    }

    // Fall back to directory name
    package_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project")
        .to_string()
}

fn parse_package_json_name(content: &str) -> Option<String> {
    // Simple extraction without full JSON parsing
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with("\"name\"") {
            if let Some(start) = line.find(':') {
                let value = line[start + 1..].trim().trim_matches(',');
                let value = value.trim_matches('"');
                if !value.is_empty() {
                    return Some(value.to_string());
                }
            }
        }
    }
    None
}

fn find_workspaces(project_path: &Path) -> Vec<std::path::PathBuf> {
    let mut workspaces = Vec::new();

    let package_json = project_path.join("package.json");
    if !package_json.exists() {
        return workspaces;
    }

    let content = match std::fs::read_to_string(&package_json) {
        Ok(c) => c,
        Err(_) => return workspaces,
    };

    // Look for "workspaces" field with common patterns
    // This is a simplified check - real implementation would parse JSON
    if !content.contains("\"workspaces\"") {
        return workspaces;
    }

    // Common workspace patterns
    let patterns = ["packages/*", "apps/*", "libs/*"];

    for pattern in patterns {
        let base_dir = project_path.join(pattern.trim_end_matches("/*"));
        if base_dir.exists() && base_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&base_dir) {
                for entry in entries.filter_map(|e| e.ok()) {
                    let path = entry.path();
                    if path.is_dir() && path.join("package.json").exists() {
                        workspaces.push(path);
                    }
                }
            }
        }
    }

    workspaces
}

/// Find Node.js entry points
pub fn find_entry_points(project_path: &Path) -> Vec<String> {
    let mut entries = Vec::new();

    // Common entry points
    let candidates = [
        "src/index.ts",
        "src/index.js",
        "src/main.ts",
        "src/main.js",
        "index.ts",
        "index.js",
        "main.ts",
        "main.js",
        "src/app.ts",
        "src/app.js",
        "app.ts",
        "app.js",
    ];

    for candidate in candidates {
        let path = project_path.join(candidate);
        if path.exists() {
            entries.push(candidate.to_string());
        }
    }

    entries
}

/// Resolve Node.js import to module ID
pub fn resolve_import_to_module(import: &str, module_ids: &[(String, String)]) -> Option<String> {
    // Node imports: "./foo", "../bar", "@scope/pkg", "lodash"
    // Strip leading ./ or ../
    let import = import.trim_start_matches("./").trim_start_matches("../");

    for (id, name) in module_ids {
        if id.ends_with(import) || import.starts_with(name) || name == import {
            return Some(id.clone());
        }
    }
    None
}

/// Count lines in Node.js module
pub fn count_lines_in_module(project_path: &Path, module_path: &str) -> u32 {
    let full_path = project_path.join(module_path);

    let mut count = 0u32;

    if full_path.is_file() {
        if let Ok(content) = std::fs::read_to_string(&full_path) {
            return content.lines().count() as u32;
        }
    }

    for entry in WalkDir::new(&full_path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| is_source_file(e.path()))
    {
        if let Ok(content) = std::fs::read_to_string(entry.path()) {
            count += content.lines().count() as u32;
        }
    }
    count
}
