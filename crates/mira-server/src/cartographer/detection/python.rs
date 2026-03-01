// crates/mira-server/src/cartographer/detection/python.rs
// Python package/module detection from project structure

use super::super::types::Module;
use crate::project_files::FileWalker;
use crate::utils::{path_to_string, relative_to};
use std::collections::HashSet;
use std::path::Path;

/// Detect Python modules from project structure
pub fn detect(project_path: &Path) -> Vec<Module> {
    let mut modules = Vec::new();

    tracing::info!("detect_python_modules: scanning {:?}", project_path);

    // Find the main package - look for pyproject.toml, setup.py, or top-level __init__.py
    let _project_name = find_project_name(project_path);

    // Walk directory looking for packages (__init__.py) and modules (.py files)
    let mut seen_dirs: HashSet<String> = HashSet::new();

    // Detect packages (dirs with __init__.py) from project root
    detect_packages_in_dir(project_path, project_path, "", &mut modules, &mut seen_dirs);

    // Also detect top-level .py files as modules
    for entry in FileWalker::new(project_path)
        .for_language("python")
        .max_depth(8)
        .walk_entries()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();

        if !(path.is_file() && path.extension().is_some_and(|e| e == "py")) {
            continue;
        }

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
            let relative = relative_to(path, project_path);
            let module_path = path_to_string(relative);
            if !seen_dirs.contains(&module_path) {
                seen_dirs.insert(module_path.clone());

                let module_name = file_name.trim_end_matches(".py");
                let module_id = module_name.to_string();

                modules.push(Module::new(module_id, module_name, module_path));
            }
        }
    }

    // If no modules found but we have a src/ layout, treat src/ contents as modules
    if modules.is_empty() {
        let src_dir = project_path.join("src");
        if src_dir.exists() {
            detect_packages_in_dir(&src_dir, project_path, "src/", &mut modules, &mut seen_dirs);
        }
    }

    tracing::info!("detect_python_modules: found {} modules", modules.len());
    for m in &modules {
        tracing::debug!("  module: {} at {}", m.id, m.path);
    }

    modules
}

/// Walk `search_dir` for Python packages (dirs with `__init__.py`).
/// Paths are relativized against `project_path`.
/// `strip_prefix` is removed from module IDs (e.g. "src/" for src-layout projects).
fn detect_packages_in_dir(
    search_dir: &Path,
    project_path: &Path,
    strip_prefix: &str,
    modules: &mut Vec<Module>,
    seen_dirs: &mut HashSet<String>,
) {
    for entry in FileWalker::new(search_dir)
        .for_language("python")
        .max_depth(8)
        .walk_entries()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let init_py = path.join("__init__.py");
        if !init_py.exists() {
            continue;
        }

        let relative = relative_to(path, project_path);
        let module_path = path_to_string(relative);
        if module_path.is_empty() || seen_dirs.contains(&module_path) {
            continue;
        }

        seen_dirs.insert(module_path.clone());

        let module_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");

        let module_id = module_path.replace(strip_prefix, "").replace('/', ".");

        modules.push(Module::new(module_id, module_name, module_path));
    }
}

fn find_project_name(project_path: &Path) -> String {
    // Try pyproject.toml first
    let pyproject = project_path.join("pyproject.toml");
    if pyproject.exists()
        && let Ok(content) = std::fs::read_to_string(&pyproject)
        && let Some(name) = parse_pyproject_name(&content)
    {
        return name;
    }

    // Try setup.py
    let setup_py = project_path.join("setup.py");
    if setup_py.exists()
        && let Ok(content) = std::fs::read_to_string(&setup_py)
        && let Some(name) = parse_setup_py_name(&content)
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

fn parse_pyproject_name(content: &str) -> Option<String> {
    let mut in_project = false;

    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_project = line == "[project]" || line == "[tool.poetry]";
        } else if in_project
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

fn parse_setup_py_name(content: &str) -> Option<String> {
    // Simple regex-free extraction: look for name="..." or name='...'
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with("name")
            && line.contains('=')
            && let Some(start) = line.find('"').or_else(|| line.find('\''))
        {
            // Safe because we found an ASCII quote character at byte position `start`
            let quote = line.as_bytes()[start] as char;
            let rest = &line[start + 1..];
            if let Some(end) = rest.find(quote) {
                return Some(rest[..end].to_string());
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
        if candidates.contains(&name)
            && let Ok(rel) = entry.path().strip_prefix(project_path)
        {
            entries.push(path_to_string(rel));
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
    super::count_lines_with_walker(project_path, module_path, "python")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // ============================================================================
    // parse_pyproject_name tests
    // ============================================================================

    #[test]
    fn test_parse_pyproject_name_project_section() {
        let content = r#"
[project]
name = "my-package"
version = "1.0.0"
"#;
        assert_eq!(
            parse_pyproject_name(content),
            Some("my-package".to_string())
        );
    }

    #[test]
    fn test_parse_pyproject_name_poetry_section() {
        let content = r#"
[tool.poetry]
name = "poetry-pkg"
version = "0.1.0"
"#;
        assert_eq!(
            parse_pyproject_name(content),
            Some("poetry-pkg".to_string())
        );
    }

    #[test]
    fn test_parse_pyproject_name_wrong_section() {
        let content = r#"
[build-system]
name = "not-this"
"#;
        assert_eq!(parse_pyproject_name(content), None);
    }

    #[test]
    fn test_parse_pyproject_name_single_quotes() {
        let content = r#"
[project]
name = 'single-quoted'
"#;
        assert_eq!(
            parse_pyproject_name(content),
            Some("single-quoted".to_string())
        );
    }

    // ============================================================================
    // parse_setup_py_name tests
    // ============================================================================

    #[test]
    fn test_parse_setup_py_name_double_quotes() {
        let content = r#"
from setuptools import setup
setup(
    name="my-setup-pkg",
    version="1.0",
)
"#;
        assert_eq!(
            parse_setup_py_name(content),
            Some("my-setup-pkg".to_string())
        );
    }

    #[test]
    fn test_parse_setup_py_name_single_quotes() {
        let content = "    name='single-setup',\n";
        assert_eq!(
            parse_setup_py_name(content),
            Some("single-setup".to_string())
        );
    }

    #[test]
    fn test_parse_setup_py_name_missing() {
        let content = "from setuptools import setup\nsetup(version='1.0')\n";
        assert_eq!(parse_setup_py_name(content), None);
    }

    // ============================================================================
    // find_project_name tests
    // ============================================================================

    #[test]
    fn test_find_project_name_from_pyproject() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("pyproject.toml"),
            "[project]\nname = \"from-pyproject\"\n",
        )
        .unwrap();
        assert_eq!(find_project_name(dir.path()), "from-pyproject");
    }

    #[test]
    fn test_find_project_name_fallback_to_dir() {
        let dir = TempDir::new().unwrap();
        let name = find_project_name(dir.path());
        // Should fall back to directory name (tempdir name)
        assert!(!name.is_empty());
    }

    // ============================================================================
    // resolve_import_to_module tests
    // ============================================================================

    #[test]
    fn test_resolve_import_exact_match() {
        let modules = vec![("mypackage.utils".to_string(), "utils".to_string())];
        assert_eq!(
            resolve_import_to_module("mypackage.utils", &modules),
            Some("mypackage.utils".to_string())
        );
    }

    #[test]
    fn test_resolve_import_prefix_match() {
        let modules = vec![("mypackage.db".to_string(), "db".to_string())];
        assert_eq!(
            resolve_import_to_module("mypackage.db.models", &modules),
            Some("mypackage.db".to_string())
        );
    }

    #[test]
    fn test_resolve_import_by_name() {
        let modules = vec![("mypackage.helpers".to_string(), "helpers".to_string())];
        assert_eq!(
            resolve_import_to_module("helpers", &modules),
            Some("mypackage.helpers".to_string())
        );
    }

    #[test]
    fn test_resolve_import_no_match() {
        let modules = vec![("mypackage.db".to_string(), "db".to_string())];
        assert_eq!(resolve_import_to_module("unknown.module", &modules), None);
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
    fn test_find_entry_points_common_files() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("main.py"), "print('hi')").unwrap();
        std::fs::write(dir.path().join("app.py"), "app = Flask()").unwrap();
        std::fs::write(dir.path().join("utils.py"), "# not an entry point").unwrap();

        let entries = find_entry_points(dir.path());
        assert!(entries.contains(&"main.py".to_string()));
        assert!(entries.contains(&"app.py".to_string()));
        assert!(!entries.contains(&"utils.py".to_string()));
    }

    #[test]
    fn test_find_entry_points_empty_project() {
        let dir = TempDir::new().unwrap();
        let entries = find_entry_points(dir.path());
        assert!(entries.is_empty());
    }

    // ============================================================================
    // detect tests
    // ============================================================================

    #[test]
    fn test_detect_package_with_init() {
        let dir = TempDir::new().unwrap();
        let pkg = dir.path().join("mypackage");
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(pkg.join("__init__.py"), "").unwrap();
        std::fs::write(pkg.join("core.py"), "def main(): pass").unwrap();

        let modules = detect(dir.path());
        assert!(modules.iter().any(|m| m.name == "mypackage"));
        assert!(modules.iter().any(|m| m.id == "mypackage"));
    }

    #[test]
    fn test_detect_nested_packages() {
        let dir = TempDir::new().unwrap();
        let pkg = dir.path().join("mypackage");
        let sub = pkg.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(pkg.join("__init__.py"), "").unwrap();
        std::fs::write(sub.join("__init__.py"), "").unwrap();
        std::fs::write(sub.join("models.py"), "class Model: pass").unwrap();

        let modules = detect(dir.path());
        assert!(modules.iter().any(|m| m.id == "mypackage"));
        assert!(modules.iter().any(|m| m.id == "mypackage.sub"));
    }

    #[test]
    fn test_detect_top_level_py_files() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("cli.py"), "import click").unwrap();
        std::fs::write(dir.path().join("config.py"), "DEBUG = True").unwrap();
        // Should skip these:
        std::fs::write(dir.path().join("setup.py"), "").unwrap();
        std::fs::write(dir.path().join("conftest.py"), "").unwrap();
        std::fs::write(dir.path().join("test_main.py"), "").unwrap();

        let modules = detect(dir.path());
        assert!(modules.iter().any(|m| m.name == "cli"));
        assert!(modules.iter().any(|m| m.name == "config"));
        assert!(!modules.iter().any(|m| m.name == "setup"));
        assert!(!modules.iter().any(|m| m.name == "conftest"));
        assert!(!modules.iter().any(|m| m.name == "test_main"));
    }

    #[test]
    fn test_detect_empty_project() {
        let dir = TempDir::new().unwrap();
        let modules = detect(dir.path());
        assert!(modules.is_empty());
    }
}
