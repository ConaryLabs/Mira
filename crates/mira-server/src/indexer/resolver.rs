// crates/mira-server/src/indexer/resolver.rs
// Cross-file import resolution for supported languages.
//
// Provides a trait and concrete implementations for resolving import paths
// (e.g., `crate::tools::core::code`) to file paths on disk.

use std::path::{Path, PathBuf};

/// The result of successfully resolving an import path to a file.
pub struct ResolvedImport {
    /// Absolute path to the file containing the imported symbol/module
    pub file_path: PathBuf,
    /// The symbol name within the file, if the import targets a specific symbol
    pub symbol_name: Option<String>,
    /// The canonical module path that was resolved (e.g., "crate::tools::core")
    pub module_path: String,
}

/// Trait for resolving import paths to file locations.
pub trait ImportResolver: Send + Sync {
    /// Attempt to resolve an import path to a file.
    ///
    /// Returns `None` if the import cannot be resolved (e.g., external crate,
    /// ambiguous path, or file not found).
    fn resolve_import(&self, import_path: &str, project_path: &Path) -> Option<ResolvedImport>;
}

/// Rust import resolver.
///
/// Resolves `crate::`, `super::`, and `self::` import paths to file paths by
/// walking the Rust module tree starting from `lib.rs` or `main.rs`.
pub struct RustImportResolver;

impl RustImportResolver {
    /// Find the crate root (lib.rs or main.rs) for a given project path.
    /// Searches common locations: src/lib.rs, src/main.rs, lib.rs, main.rs.
    fn find_crate_root(project_path: &Path) -> Option<PathBuf> {
        let candidates = [
            project_path.join("src").join("lib.rs"),
            project_path.join("src").join("main.rs"),
            project_path.join("lib.rs"),
            project_path.join("main.rs"),
        ];
        candidates.into_iter().find(|p| p.exists())
    }

    /// Resolve a module path segment sequence to a file path relative to `src_root`.
    ///
    /// For a module path `["tools", "core", "code"]`, tries:
    ///   1. `src_root/tools/core/code.rs`
    ///   2. `src_root/tools/core/code/mod.rs`
    fn resolve_segments(src_root: &Path, segments: &[&str]) -> Option<PathBuf> {
        if segments.is_empty() {
            return None;
        }

        let mut path = src_root.to_path_buf();
        for segment in segments {
            path = path.join(segment);
        }

        // Try as a file: path.rs
        let as_file = path.with_extension("rs");
        if as_file.exists() && as_file.starts_with(src_root) {
            return Some(as_file);
        }

        // Try as a module directory: path/mod.rs
        let as_mod = path.join("mod.rs");
        if as_mod.exists() && as_mod.starts_with(src_root) {
            return Some(as_mod);
        }

        None
    }

    /// Split an import path into (module segments, optional symbol name).
    ///
    /// For `crate::tools::core::Code`, returns `(["tools", "core"], Some("Code"))`.
    /// Uses a heuristic: the last segment is a symbol if it starts with an uppercase letter.
    fn split_import(import_path: &str) -> (Vec<&str>, Option<&str>) {
        let segments: Vec<&str> = import_path.split("::").collect();
        if segments.is_empty() {
            return (vec![], None);
        }

        let last = match segments.last() {
            Some(s) => *s,
            None => return (vec![], None),
        };
        // Heuristic: PascalCase or SCREAMING_SNAKE_CASE => symbol name
        let is_symbol = last
            .chars()
            .next()
            .map(|c| c.is_uppercase())
            .unwrap_or(false);

        if is_symbol && segments.len() > 1 {
            let module_segs = &segments[..segments.len() - 1];
            (module_segs.to_vec(), Some(last))
        } else {
            (segments, None)
        }
    }
}

impl ImportResolver for RustImportResolver {
    fn resolve_import(&self, import_path: &str, project_path: &Path) -> Option<ResolvedImport> {
        let crate_root = Self::find_crate_root(project_path)?;
        let src_root = crate_root.parent()?;

        // Strip leading prefix to get the module path
        let rest = if let Some(s) = import_path.strip_prefix("crate::") {
            s
        } else if import_path.starts_with("super::") || import_path.starts_with("self::") {
            // super:: and self:: are relative — we can't resolve without call-site context
            return None;
        } else if import_path.starts_with("std::")
            || import_path.starts_with("core::")
            || import_path.starts_with("alloc::")
        {
            // Standard library — not resolvable to a local file
            return None;
        } else {
            // External crate or bare path — not resolvable
            return None;
        };

        let (module_segs, symbol_name) = Self::split_import(rest);
        if module_segs.is_empty() {
            return None;
        }

        let file_path = Self::resolve_segments(src_root, &module_segs)?;
        let module_path = format!("crate::{}", module_segs.join("::"));

        Some(ResolvedImport {
            file_path,
            symbol_name: symbol_name.map(|s| s.to_string()),
            module_path,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn make_project(files: &[(&str, &str)]) -> TempDir {
        let dir = TempDir::new().unwrap();
        for (rel_path, content) in files {
            let full = dir.path().join(rel_path);
            if let Some(parent) = full.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(full, content).unwrap();
        }
        dir
    }

    #[test]
    fn resolves_crate_module_as_file() {
        let dir = make_project(&[
            ("src/lib.rs", "mod tools;"),
            ("src/tools.rs", "pub fn foo() {}"),
        ]);
        let resolver = RustImportResolver;
        let result = resolver.resolve_import("crate::tools", dir.path());
        assert!(result.is_some());
        let r = result.unwrap();
        assert!(r.file_path.ends_with("src/tools.rs"));
        assert_eq!(r.module_path, "crate::tools");
        assert!(r.symbol_name.is_none());
    }

    #[test]
    fn resolves_crate_module_as_mod_rs() {
        let dir = make_project(&[
            ("src/lib.rs", "mod tools;"),
            ("src/tools/mod.rs", "pub fn bar() {}"),
        ]);
        let resolver = RustImportResolver;
        let result = resolver.resolve_import("crate::tools", dir.path());
        assert!(result.is_some());
        let r = result.unwrap();
        assert!(r.file_path.ends_with("src/tools/mod.rs"));
    }

    #[test]
    fn resolves_nested_module() {
        let dir = make_project(&[
            ("src/lib.rs", "mod tools;"),
            ("src/tools/core.rs", "pub fn baz() {}"),
        ]);
        let resolver = RustImportResolver;
        let result = resolver.resolve_import("crate::tools::core", dir.path());
        assert!(result.is_some());
        let r = result.unwrap();
        assert!(r.file_path.ends_with("src/tools/core.rs"));
        assert_eq!(r.module_path, "crate::tools::core");
    }

    #[test]
    fn resolves_symbol_name_from_uppercase() {
        let dir = make_project(&[
            ("src/lib.rs", "mod tools;"),
            ("src/tools.rs", "pub struct MyTool;"),
        ]);
        let resolver = RustImportResolver;
        let result = resolver.resolve_import("crate::tools::MyTool", dir.path());
        assert!(result.is_some());
        let r = result.unwrap();
        assert!(r.file_path.ends_with("src/tools.rs"));
        assert_eq!(r.symbol_name, Some("MyTool".to_string()));
        assert_eq!(r.module_path, "crate::tools");
    }

    #[test]
    fn returns_none_for_std_import() {
        let dir = make_project(&[("src/lib.rs", "")]);
        let resolver = RustImportResolver;
        assert!(
            resolver
                .resolve_import("std::collections::HashMap", dir.path())
                .is_none()
        );
    }

    #[test]
    fn returns_none_for_external_crate() {
        let dir = make_project(&[("src/lib.rs", "")]);
        let resolver = RustImportResolver;
        assert!(
            resolver
                .resolve_import("serde::Deserialize", dir.path())
                .is_none()
        );
    }

    #[test]
    fn returns_none_when_file_not_found() {
        let dir = make_project(&[("src/lib.rs", "")]);
        let resolver = RustImportResolver;
        assert!(
            resolver
                .resolve_import("crate::nonexistent::module", dir.path())
                .is_none()
        );
    }

    #[test]
    fn uses_main_rs_as_crate_root() {
        let dir = make_project(&[
            ("src/main.rs", "mod config;"),
            ("src/config.rs", "pub const X: u32 = 1;"),
        ]);
        let resolver = RustImportResolver;
        let result = resolver.resolve_import("crate::config", dir.path());
        assert!(result.is_some());
        assert!(result.unwrap().file_path.ends_with("src/config.rs"));
    }
}
