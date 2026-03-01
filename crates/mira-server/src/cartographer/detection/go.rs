// crates/mira-server/src/cartographer/detection/go.rs
// Go package detection from project structure

use super::super::types::Module;
use crate::project_files::FileWalker;
use crate::utils::{path_to_string, relative_to};
use std::collections::HashSet;
use std::path::Path;

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

        let relative = relative_to(path, project_path);
        let module_path = path_to_string(relative);

        if seen_dirs.contains(&module_path) {
            continue;
        }
        seen_dirs.insert(module_path.clone());

        let package_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("main");

        // Create module ID: module_name/relative_path or just package name for root
        let module_id = if module_path.is_empty() {
            module_name.clone()
        } else {
            format!("{}/{}", module_name, module_path)
        };

        let path = if module_path.is_empty() {
            ".".to_string()
        } else {
            module_path
        };
        modules.push(Module::new(module_id, package_name, path));
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
    if go_mod.exists()
        && let Ok(content) = std::fs::read_to_string(&go_mod)
        && let Some(name) = parse_go_mod_name(&content)
    {
        return name;
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
            let name = entry
                .path()
                .file_name()
                .unwrap_or_default()
                .to_string_lossy();
            if name == "main.go"
                && let Ok(rel) = path.strip_prefix(project_path)
            {
                entries.push(path_to_string(rel));
            }
        }
    }

    // Also check cmd/ subdirectories
    let cmd_dir = project_path.join("cmd");
    if cmd_dir.exists()
        && let Ok(cmd_entries) = std::fs::read_dir(&cmd_dir)
    {
        for entry in cmd_entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir() {
                let main_go = path.join("main.go");
                if main_go.exists()
                    && let Ok(rel) = main_go.strip_prefix(project_path)
                {
                    entries.push(path_to_string(rel));
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
#[allow(clippy::manual_inspect)]
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
            if path.extension().is_some_and(|e| e == "go")
                && let Ok(content) = std::fs::read_to_string(&path)
            {
                count += content.lines().count() as u32;
            }
        }
    }

    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_go_mod(dir: &Path, module_name: &str) {
        std::fs::write(
            dir.join("go.mod"),
            format!("module {module_name}\n\ngo 1.21\n"),
        )
        .unwrap();
    }

    // ============================================================================
    // parse_go_mod_name tests
    // ============================================================================

    #[test]
    fn test_parse_go_mod_name_simple() {
        let content = "module github.com/user/myapp\n\ngo 1.21\n";
        assert_eq!(
            parse_go_mod_name(content),
            Some("github.com/user/myapp".to_string())
        );
    }

    #[test]
    fn test_parse_go_mod_name_with_version() {
        let content = "module github.com/user/myapp/v2\n\ngo 1.21\n";
        assert_eq!(
            parse_go_mod_name(content),
            Some("github.com/user/myapp/v2".to_string())
        );
    }

    #[test]
    fn test_parse_go_mod_name_missing() {
        let content = "go 1.21\n\nrequire (\n)\n";
        assert_eq!(parse_go_mod_name(content), None);
    }

    // ============================================================================
    // find_module_name tests
    // ============================================================================

    #[test]
    fn test_find_module_name_from_go_mod() {
        let dir = TempDir::new().unwrap();
        write_go_mod(dir.path(), "github.com/user/myapp");
        assert_eq!(find_module_name(dir.path()), "github.com/user/myapp");
    }

    #[test]
    fn test_find_module_name_fallback_to_dir() {
        let dir = TempDir::new().unwrap();
        let name = find_module_name(dir.path());
        assert!(!name.is_empty());
    }

    // ============================================================================
    // resolve_import_to_module tests
    // ============================================================================

    #[test]
    fn test_resolve_import_exact_match() {
        let modules = vec![("github.com/user/app/pkg/db".to_string(), "db".to_string())];
        assert_eq!(
            resolve_import_to_module("github.com/user/app/pkg/db", &modules),
            Some("github.com/user/app/pkg/db".to_string())
        );
    }

    #[test]
    fn test_resolve_import_by_name() {
        let modules = vec![(
            "github.com/user/app/internal/auth".to_string(),
            "auth".to_string(),
        )];
        assert_eq!(
            resolve_import_to_module("auth", &modules),
            Some("github.com/user/app/internal/auth".to_string())
        );
    }

    #[test]
    fn test_resolve_import_no_match() {
        let modules = vec![("github.com/user/app/pkg/db".to_string(), "db".to_string())];
        assert_eq!(resolve_import_to_module("fmt", &modules), None);
    }

    #[test]
    fn test_resolve_import_empty_modules() {
        let modules: Vec<(String, String)> = vec![];
        assert_eq!(resolve_import_to_module("anything", &modules), None);
    }

    // ============================================================================
    // find_entry_points tests
    // ============================================================================

    #[test]
    fn test_find_entry_points_root_main() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("main.go"), "package main\nfunc main() {}").unwrap();

        let entries = find_entry_points(dir.path());
        assert!(entries.contains(&"main.go".to_string()));
    }

    #[test]
    fn test_find_entry_points_cmd_dirs() {
        let dir = TempDir::new().unwrap();
        let cmd_server = dir.path().join("cmd/server");
        let cmd_cli = dir.path().join("cmd/cli");
        std::fs::create_dir_all(&cmd_server).unwrap();
        std::fs::create_dir_all(&cmd_cli).unwrap();
        std::fs::write(cmd_server.join("main.go"), "package main").unwrap();
        std::fs::write(cmd_cli.join("main.go"), "package main").unwrap();

        let entries = find_entry_points(dir.path());
        assert!(entries.iter().any(|e| e.contains("cmd/server/main.go")));
        assert!(entries.iter().any(|e| e.contains("cmd/cli/main.go")));
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
    fn test_detect_simple_project() {
        let dir = TempDir::new().unwrap();
        write_go_mod(dir.path(), "github.com/user/myapp");
        std::fs::write(dir.path().join("main.go"), "package main\nfunc main() {}").unwrap();

        let modules = detect(dir.path());
        assert!(!modules.is_empty());
        assert!(modules.iter().any(|m| m.id == "github.com/user/myapp"));
    }

    #[test]
    fn test_detect_with_subpackage() {
        let dir = TempDir::new().unwrap();
        write_go_mod(dir.path(), "github.com/user/myapp");
        std::fs::write(dir.path().join("main.go"), "package main").unwrap();

        // Create a direct child package (FileWalker yields top-level subdirs)
        let pkg = dir.path().join("handlers");
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(pkg.join("routes.go"), "package handlers").unwrap();

        let modules = detect(dir.path());
        assert!(
            modules.iter().any(|m| m.name == "handlers"),
            "expected a module named 'handlers', got: {:?}",
            modules.iter().map(|m| &m.id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_detect_skips_test_only_dirs() {
        let dir = TempDir::new().unwrap();
        write_go_mod(dir.path(), "github.com/user/myapp");

        let pkg = dir.path().join("pkg/testutil");
        std::fs::create_dir_all(&pkg).unwrap();
        // Directory with only _test.go files should not be detected
        std::fs::write(pkg.join("helpers_test.go"), "package testutil").unwrap();

        let modules = detect(dir.path());
        assert!(!modules.iter().any(|m| m.name == "testutil"));
    }

    #[test]
    fn test_detect_empty_project() {
        let dir = TempDir::new().unwrap();
        let modules = detect(dir.path());
        assert!(modules.is_empty());
    }

    // ============================================================================
    // count_lines_in_module tests
    // ============================================================================

    #[test]
    fn test_count_lines_root_package() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("main.go"),
            "package main\n\nfunc main() {\n}\n",
        )
        .unwrap();

        let count = count_lines_in_module(dir.path(), ".");
        assert_eq!(count, 4);
    }

    #[test]
    fn test_count_lines_subpackage() {
        let dir = TempDir::new().unwrap();
        let pkg = dir.path().join("pkg/db");
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(pkg.join("db.go"), "package db\n\nfunc Open() {}\n").unwrap();
        std::fs::write(pkg.join("db_test.go"), "package db\n\nfunc TestOpen() {}\n").unwrap();

        let count = count_lines_in_module(dir.path(), "pkg/db");
        // Should count both .go files (including test files for line counting)
        assert!(count > 0);
    }
}
