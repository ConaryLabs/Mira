// crates/mira-server/src/cartographer/detection/node.rs
// Node.js/TypeScript module detection from project structure

use super::super::types::Module;
use crate::project_files::FileWalker;
use crate::utils::{path_to_string, relative_to};
use std::collections::HashSet;
use std::path::Path;

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

    for entry in FileWalker::new(&search_root)
        .for_language("node")
        .max_depth(8)
        .walk_entries()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        let relative = relative_to(path, project_path);

        // Directory with index.ts/js is a module
        if path.is_dir() {
            let has_index = has_index_file(path);
            if has_index {
                let module_path = path_to_string(relative);
                if !seen_dirs.contains(&module_path) && !module_path.is_empty() {
                    seen_dirs.insert(module_path.clone());

                    let module_name = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("unknown");

                    // Create module ID: package_name/relative_path
                    let pkg_relative = relative_to(path, package_path);
                    let module_id = if pkg_relative.as_os_str().is_empty() {
                        package_name.to_string()
                    } else {
                        format!("{}/{}", package_name, pkg_relative.to_string_lossy())
                    };

                    modules.push(Module::new(module_id, module_name, module_path));
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
                let module_path = path_to_string(relative);
                if !seen_dirs.contains(&module_path) {
                    seen_dirs.insert(module_path.clone());

                    let module_name = file_name
                        .trim_end_matches(".ts")
                        .trim_end_matches(".tsx")
                        .trim_end_matches(".js")
                        .trim_end_matches(".jsx");

                    let module_id = format!("{}/{}", package_name, module_name);

                    modules.push(Module::new(module_id, module_name, module_path));
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

        modules.push(Module::new(package_name, package_name, module_path));
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
    if package_json.exists()
        && let Ok(content) = std::fs::read_to_string(&package_json)
        && let Some(name) = parse_package_json_name(&content)
    {
        return name;
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
        if line.starts_with("\"name\"")
            && let Some(start) = line.find(':')
        {
            let value = line[start + 1..].trim().trim_matches(',');
            let value = value.trim_matches('"');
            if !value.is_empty() {
                return Some(value.to_string());
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
        if base_dir.exists()
            && base_dir.is_dir()
            && let Ok(entries) = std::fs::read_dir(&base_dir)
        {
            for entry in entries.filter_map(|e| e.ok()) {
                let path = entry.path();
                if path.is_dir() && path.join("package.json").exists() {
                    workspaces.push(path);
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

/// Count lines in Node.js module (delegates to shared walker-based helper)
pub fn count_lines_in_module(project_path: &Path, module_path: &str) -> u32 {
    super::count_lines_with_walker(project_path, module_path, "node")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_package_json(dir: &Path, name: &str) {
        std::fs::write(
            dir.join("package.json"),
            format!("{{\n  \"name\": \"{name}\",\n  \"version\": \"1.0.0\"\n}}\n"),
        )
        .unwrap();
    }

    // ============================================================================
    // parse_package_json_name tests
    // ============================================================================

    #[test]
    fn test_parse_package_json_name_simple() {
        let content = "{\n  \"name\": \"my-app\",\n  \"version\": \"1.0.0\"\n}";
        assert_eq!(parse_package_json_name(content), Some("my-app".to_string()));
    }

    #[test]
    fn test_parse_package_json_name_scoped() {
        let content = "{\n  \"name\": \"@scope/my-lib\",\n  \"version\": \"1.0.0\"\n}";
        assert_eq!(
            parse_package_json_name(content),
            Some("@scope/my-lib".to_string())
        );
    }

    #[test]
    fn test_parse_package_json_name_missing() {
        let content = "{\n  \"version\": \"1.0.0\"\n}";
        assert_eq!(parse_package_json_name(content), None);
    }

    // ============================================================================
    // find_workspaces tests
    // ============================================================================

    #[test]
    fn test_find_workspaces_monorepo() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            "{\n  \"name\": \"mono\",\n  \"workspaces\": [\"packages/*\"]\n}",
        )
        .unwrap();

        let pkg_a = dir.path().join("packages/pkg-a");
        let pkg_b = dir.path().join("packages/pkg-b");
        std::fs::create_dir_all(&pkg_a).unwrap();
        std::fs::create_dir_all(&pkg_b).unwrap();
        write_package_json(&pkg_a, "pkg-a");
        write_package_json(&pkg_b, "pkg-b");

        let workspaces = find_workspaces(dir.path());
        assert_eq!(workspaces.len(), 2);
    }

    #[test]
    fn test_find_workspaces_no_workspaces_field() {
        let dir = TempDir::new().unwrap();
        write_package_json(dir.path(), "single-pkg");
        let workspaces = find_workspaces(dir.path());
        assert!(workspaces.is_empty());
    }

    #[test]
    fn test_find_workspaces_no_package_json() {
        let dir = TempDir::new().unwrap();
        let workspaces = find_workspaces(dir.path());
        assert!(workspaces.is_empty());
    }

    // ============================================================================
    // resolve_import_to_module tests
    // ============================================================================

    #[test]
    fn test_resolve_import_relative() {
        let modules = vec![("my-app/utils".to_string(), "utils".to_string())];
        assert_eq!(
            resolve_import_to_module("./utils", &modules),
            Some("my-app/utils".to_string())
        );
    }

    #[test]
    fn test_resolve_import_parent_relative() {
        let modules = vec![("my-app/helpers".to_string(), "helpers".to_string())];
        assert_eq!(
            resolve_import_to_module("../helpers", &modules),
            Some("my-app/helpers".to_string())
        );
    }

    #[test]
    fn test_resolve_import_by_name() {
        let modules = vec![("my-app/config".to_string(), "config".to_string())];
        assert_eq!(
            resolve_import_to_module("config", &modules),
            Some("my-app/config".to_string())
        );
    }

    #[test]
    fn test_resolve_import_no_match() {
        let modules = vec![("my-app/db".to_string(), "db".to_string())];
        assert_eq!(resolve_import_to_module("unknown", &modules), None);
    }

    #[test]
    fn test_resolve_import_empty_modules() {
        let modules: Vec<(String, String)> = vec![];
        assert_eq!(resolve_import_to_module("./anything", &modules), None);
    }

    // ============================================================================
    // find_entry_points tests
    // ============================================================================

    #[test]
    fn test_find_entry_points_src_index() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("index.ts"), "export {}").unwrap();

        let entries = find_entry_points(dir.path());
        assert!(entries.contains(&"src/index.ts".to_string()));
    }

    #[test]
    fn test_find_entry_points_root_js() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("index.js"), "module.exports = {}").unwrap();
        std::fs::write(dir.path().join("app.js"), "const app = express()").unwrap();

        let entries = find_entry_points(dir.path());
        assert!(entries.contains(&"index.js".to_string()));
        assert!(entries.contains(&"app.js".to_string()));
    }

    #[test]
    fn test_find_entry_points_empty() {
        let dir = TempDir::new().unwrap();
        let entries = find_entry_points(dir.path());
        assert!(entries.is_empty());
    }

    // ============================================================================
    // detect tests
    // ============================================================================

    #[test]
    fn test_detect_single_package_with_src() {
        let dir = TempDir::new().unwrap();
        write_package_json(dir.path(), "my-app");
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("index.ts"), "export {}").unwrap();
        std::fs::write(src.join("utils.ts"), "export function helper() {}").unwrap();

        let modules = detect(dir.path());
        assert!(!modules.is_empty());
        // Should find utils as a module (index.ts is skipped)
        assert!(modules.iter().any(|m| m.name == "utils"));
    }

    #[test]
    fn test_detect_with_index_subdirectory() {
        let dir = TempDir::new().unwrap();
        write_package_json(dir.path(), "my-app");
        let src = dir.path().join("src");
        let components = src.join("components");
        std::fs::create_dir_all(&components).unwrap();
        std::fs::write(components.join("index.ts"), "export {}").unwrap();

        let modules = detect(dir.path());
        assert!(modules.iter().any(|m| m.name == "components"));
    }

    #[test]
    fn test_detect_skips_test_files() {
        let dir = TempDir::new().unwrap();
        write_package_json(dir.path(), "my-app");
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("app.ts"), "export {}").unwrap();
        std::fs::write(src.join("app.test.ts"), "test('works', () => {})").unwrap();
        std::fs::write(src.join("app.spec.ts"), "describe('app', () => {})").unwrap();

        let modules = detect(dir.path());
        let names: Vec<&str> = modules.iter().map(|m| m.name.as_str()).collect();
        assert!(names.contains(&"app"));
        // test/spec files should not appear as modules
        assert!(!modules.iter().any(|m| m.name.contains("test")));
        assert!(!modules.iter().any(|m| m.name.contains("spec")));
    }

    #[test]
    fn test_detect_empty_project() {
        let dir = TempDir::new().unwrap();
        let modules = detect(dir.path());
        // Even empty projects get a fallback module
        assert!(!modules.is_empty());
    }
}
