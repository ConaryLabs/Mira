// crates/mira-server/src/cartographer/detection/rust.rs
// Rust module detection from project structure

use super::super::types::Module;
use crate::project_files::FileWalker;
use crate::utils::{path_to_string, relative_to};
use std::collections::HashSet;
use std::path::Path;

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
        } else if in_package
            && line.starts_with("name")
            && let Some(name) = line.split('=').nth(1)
        {
            let name = name.trim().trim_matches('"').trim_matches('\'');
            if !name.is_empty() {
                return Some(name.to_string());
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
        let relative = relative_to(&path, project_path);
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        // Entry points become top-level modules
        if file_name == "lib.rs" || file_name == "main.rs" {
            let module_path = relative.parent().map(path_to_string).unwrap_or_default();

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
                    detected_patterns: None,
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
                let module_path = path_to_string(relative_to(parent, project_path));

                // Create module ID: crate_name/relative_module_path
                let src_relative = relative_to(parent, src_dir);
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
                        detected_patterns: None,
                    });
                }
            }
        }
        // Regular .rs files in src/ are also modules
        else if path.parent() == Some(src_dir) && file_name != "lib.rs" && file_name != "main.rs"
        {
            let module_name = file_name.trim_end_matches(".rs");
            let module_id = format!("{}/{}", crate_name, module_name);
            let module_path = path_to_string(relative);

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
                    detected_patterns: None,
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
        if (name == "main.rs" || name == "lib.rs")
            && let Ok(rel) = path.strip_prefix(project_path)
        {
            entries.push(path_to_string(rel));
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
    super::count_lines_with_walker(project_path, module_path, "rust")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    // ============================================================================
    // parse_crate_name tests
    // ============================================================================

    fn create_cargo_toml(dir: &Path, content: &str) -> std::path::PathBuf {
        let cargo_path = dir.join("Cargo.toml");
        let mut file = std::fs::File::create(&cargo_path).unwrap();
        file.write_all(content.as_bytes()).unwrap();
        cargo_path
    }

    #[test]
    fn test_parse_crate_name_simple() {
        let dir = TempDir::new().unwrap();
        let cargo = create_cargo_toml(
            dir.path(),
            r#"
[package]
name = "my-crate"
version = "0.1.0"
"#,
        );
        assert_eq!(parse_crate_name(&cargo), Some("my-crate".to_string()));
    }

    #[test]
    fn test_parse_crate_name_single_quotes() {
        let dir = TempDir::new().unwrap();
        let cargo = create_cargo_toml(
            dir.path(),
            r#"
[package]
name = 'single-quoted'
"#,
        );
        assert_eq!(parse_crate_name(&cargo), Some("single-quoted".to_string()));
    }

    #[test]
    fn test_parse_crate_name_with_other_sections() {
        let dir = TempDir::new().unwrap();
        let cargo = create_cargo_toml(
            dir.path(),
            r#"
[workspace]
members = ["crate-a", "crate-b"]

[package]
name = "workspace-root"
version = "0.1.0"

[dependencies]
serde = "1.0"
"#,
        );
        assert_eq!(parse_crate_name(&cargo), Some("workspace-root".to_string()));
    }

    #[test]
    fn test_parse_crate_name_no_package_section() {
        let dir = TempDir::new().unwrap();
        let cargo = create_cargo_toml(
            dir.path(),
            r#"
[workspace]
members = ["a", "b"]
"#,
        );
        assert_eq!(parse_crate_name(&cargo), None);
    }

    #[test]
    fn test_parse_crate_name_nonexistent_file() {
        let path = Path::new("/nonexistent/Cargo.toml");
        assert_eq!(parse_crate_name(path), None);
    }

    // ============================================================================
    // is_workspace tests
    // ============================================================================

    #[test]
    fn test_is_workspace_true() {
        let dir = TempDir::new().unwrap();
        let cargo = create_cargo_toml(
            dir.path(),
            r#"
[workspace]
members = ["crate-a", "crate-b"]
"#,
        );
        assert!(is_workspace(&cargo));
    }

    #[test]
    fn test_is_workspace_false() {
        let dir = TempDir::new().unwrap();
        let cargo = create_cargo_toml(
            dir.path(),
            r#"
[package]
name = "not-a-workspace"
version = "0.1.0"
"#,
        );
        assert!(!is_workspace(&cargo));
    }

    #[test]
    fn test_is_workspace_nonexistent_file() {
        let path = Path::new("/nonexistent/Cargo.toml");
        assert!(!is_workspace(path));
    }

    // ============================================================================
    // resolve_import_to_module tests
    // ============================================================================

    #[test]
    fn test_resolve_import_crate_prefix() {
        let modules = vec![
            ("mira/search".to_string(), "search".to_string()),
            ("mira/db".to_string(), "db".to_string()),
        ];
        let result = resolve_import_to_module("crate::search::semantic", &modules);
        assert_eq!(result, Some("mira/search".to_string()));
    }

    #[test]
    fn test_resolve_import_super_prefix() {
        let modules = vec![("mira/utils".to_string(), "utils".to_string())];
        let result = resolve_import_to_module("super::utils", &modules);
        assert_eq!(result, Some("mira/utils".to_string()));
    }

    #[test]
    fn test_resolve_import_direct_match() {
        let modules = vec![("mira/config".to_string(), "config".to_string())];
        let result = resolve_import_to_module("config::Settings", &modules);
        assert_eq!(result, Some("mira/config".to_string()));
    }

    #[test]
    fn test_resolve_import_no_match() {
        let modules = vec![("mira/search".to_string(), "search".to_string())];
        let result = resolve_import_to_module("unknown::module", &modules);
        assert_eq!(result, None);
    }

    #[test]
    fn test_resolve_import_empty_modules() {
        let modules: Vec<(String, String)> = vec![];
        let result = resolve_import_to_module("crate::foo", &modules);
        assert_eq!(result, None);
    }

    // ============================================================================
    // find_entry_points tests
    // ============================================================================

    #[test]
    fn test_find_entry_points_lib_and_main() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("lib.rs"), "// lib").unwrap();
        std::fs::write(src.join("main.rs"), "fn main() {}").unwrap();
        std::fs::write(src.join("utils.rs"), "// utils").unwrap();

        let entries = find_entry_points(dir.path());
        assert!(entries.contains(&"src/lib.rs".to_string()));
        assert!(entries.contains(&"src/main.rs".to_string()));
        assert!(!entries.contains(&"src/utils.rs".to_string()));
    }

    #[test]
    fn test_find_entry_points_nested_crates() {
        let dir = TempDir::new().unwrap();
        let crate_a = dir.path().join("crates/a/src");
        let crate_b = dir.path().join("crates/b/src");
        std::fs::create_dir_all(&crate_a).unwrap();
        std::fs::create_dir_all(&crate_b).unwrap();
        std::fs::write(crate_a.join("lib.rs"), "// a").unwrap();
        std::fs::write(crate_b.join("main.rs"), "fn main() {}").unwrap();

        let entries = find_entry_points(dir.path());
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().any(|e| e.contains("crates/a/src/lib.rs")));
        assert!(entries.iter().any(|e| e.contains("crates/b/src/main.rs")));
    }

    #[test]
    fn test_find_entry_points_empty_project() {
        let dir = TempDir::new().unwrap();
        let entries = find_entry_points(dir.path());
        assert!(entries.is_empty());
    }

    // ============================================================================
    // detect tests with temporary directories
    // ============================================================================

    #[test]
    fn test_detect_simple_crate() {
        let dir = TempDir::new().unwrap();

        // Create Cargo.toml
        create_cargo_toml(
            dir.path(),
            r#"
[package]
name = "test-crate"
version = "0.1.0"
"#,
        );

        // Create src directory with lib.rs
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("lib.rs"), "pub mod utils;").unwrap();
        std::fs::write(src.join("utils.rs"), "pub fn helper() {}").unwrap();

        let modules = detect(dir.path());
        assert!(!modules.is_empty());
        assert!(modules.iter().any(|m| m.id == "test-crate"));
    }

    #[test]
    fn test_detect_with_mod_rs() {
        let dir = TempDir::new().unwrap();

        create_cargo_toml(
            dir.path(),
            r#"
[package]
name = "modtest"
version = "0.1.0"
"#,
        );

        let src = dir.path().join("src");
        let submod = src.join("submodule");
        std::fs::create_dir_all(&submod).unwrap();
        std::fs::write(src.join("lib.rs"), "mod submodule;").unwrap();
        std::fs::write(submod.join("mod.rs"), "pub fn foo() {}").unwrap();

        let modules = detect(dir.path());
        assert!(modules.iter().any(|m| m.name == "submodule"));
    }

    #[test]
    fn test_detect_empty_project() {
        let dir = TempDir::new().unwrap();
        let modules = detect(dir.path());
        assert!(modules.is_empty());
    }
}
