//! Unified file walking with configurable filtering and .gitignore support.
//!
//! This module provides a `FileWalker` builder for walking project files with
//! consistent filtering across the codebase. Supports both `.gitignore`-aware
//! walking (via `ignore::WalkBuilder`) and simple directory walking (via
//! `walkdir::WalkDir`).

// crates/mira-server/src/project_files/walker.rs
use ::ignore as ignore_crate;
use anyhow::{Context, Result, anyhow};
use std::path::{Path, PathBuf};
use walkdir;

/// Unified entry type for both ignore and walkdir backends.
#[derive(Debug)]
pub enum Entry {
    Ignore(ignore_crate::DirEntry),
    WalkDir(walkdir::DirEntry),
}

impl Entry {
    pub fn path(&self) -> &Path {
        match self {
            Entry::Ignore(e) => e.path(),
            Entry::WalkDir(e) => e.path(),
        }
    }

    pub fn file_type(&self) -> Option<std::fs::FileType> {
        match self {
            Entry::Ignore(e) => e.file_type(),
            Entry::WalkDir(e) => Some(e.file_type()),
        }
    }
}

/// Configuration for file walking with builder pattern.
#[derive(Debug, Clone)]
pub struct FileWalker {
    /// Root path to walk from
    path: PathBuf,
    /// Whether to follow symbolic links
    follow_links: bool,
    /// Whether to respect .gitignore files
    use_gitignore: bool,
    /// File extensions to include (e.g., "rs", "py")
    extensions: Vec<&'static str>,
    /// Language for language-specific filtering
    language: Option<&'static str>,
    /// Skip hidden files/directories (starting with .)
    skip_hidden: bool,
    /// Maximum depth to traverse (None = unlimited)
    max_depth: Option<usize>,
}

impl FileWalker {
    /// Create a new file walker for the given path.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            follow_links: true,
            use_gitignore: true,
            extensions: Vec::new(),
            language: None,
            skip_hidden: true,
            max_depth: None,
        }
    }

    /// Set whether to follow symbolic links (default: true).
    pub fn follow_links(mut self, follow: bool) -> Self {
        self.follow_links = follow;
        self
    }

    /// Set whether to respect .gitignore files (default: true).
    pub fn use_gitignore(mut self, use_gitignore: bool) -> Self {
        self.use_gitignore = use_gitignore;
        self
    }

    /// Add a file extension to filter by (e.g., "rs" for Rust files).
    /// If no extensions are specified, all files are included.
    pub fn with_extension(mut self, ext: &'static str) -> Self {
        self.extensions.push(ext);
        self
    }

    /// Set language for language-specific filtering and extension defaults.
    /// This also enables language-specific directory skipping.
    pub fn for_language(mut self, lang: &'static str) -> Self {
        self.language = Some(lang);
        // Set default extensions for common languages
        match lang {
            "rust" => self.extensions.push("rs"),
            "python" => self.extensions.push("py"),
            "typescript" => self.extensions.push("ts"),
            "javascript" => self.extensions.push("js"),
            "go" => self.extensions.push("go"),
            _ => {}
        }
        self
    }

    /// Set whether to skip hidden files/directories (default: true).
    pub fn skip_hidden(mut self, skip: bool) -> Self {
        self.skip_hidden = skip;
        self
    }

    /// Set maximum depth to traverse (default: None = unlimited).
    pub fn max_depth(mut self, depth: usize) -> Self {
        self.max_depth = Some(depth);
        self
    }

    /// Check if a file should be included based on extension filtering.
    fn should_include_file(&self, path: &Path) -> bool {
        if self.extensions.is_empty() {
            return true;
        }
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| self.extensions.contains(&ext))
            .unwrap_or(false)
    }

    /// Walk files and return an iterator over absolute paths.
    pub fn walk_paths(&self) -> impl Iterator<Item = Result<PathBuf>> + '_ {
        let _root = self.path.clone();
        let walker = self.create_walker();
        walker.filter_map(move |entry| {
            let entry = match entry {
                Ok(entry) => entry,
                Err(e) => return Some(Err(anyhow!("Walk error: {}", e))),
            };

            // Skip directories (and entries without file type, e.g. stdin)
            if !entry.file_type().is_some_and(|ft| ft.is_file()) {
                return None;
            }

            // Check extension filtering
            if !self.should_include_file(entry.path()) {
                return None;
            }

            Some(Ok(entry.path().to_path_buf()))
        })
    }

    /// Walk files and return an iterator over relative paths (as strings).
    /// Paths are relative to the walker's root path.
    pub fn walk_relative(&self) -> impl Iterator<Item = Result<String>> + '_ {
        let root = self.path.clone();
        self.walk_paths().map(move |path_result| {
            path_result.and_then(|path| {
                path.strip_prefix(&root)
                    .map_err(|e| anyhow!("Failed to strip prefix: {}", e))
                    .map(|p| p.to_string_lossy().to_string())
            })
        })
    }

    /// Walk files and directories, returning an iterator over unified Entry types.
    /// This allows inspecting both files and directories (e.g., for package detection).
    pub fn walk_entries(&self) -> Box<dyn Iterator<Item = Result<Entry>> + '_> {
        self.create_walker()
    }

    /// Create the appropriate walker based on configuration.
    /// Returns an iterator over unified Entry types.
    fn create_walker(&self) -> Box<dyn Iterator<Item = Result<Entry>>> {
        let skip_hidden = self.skip_hidden;
        let language = self.language;

        if self.use_gitignore {
            let extra_patterns = crate::config::ignore::load_project_ignore_patterns(&self.path);
            let predicate = move |name: &str| {
                if skip_hidden && name.starts_with('.') {
                    return true;
                }
                if let Some(lang) = language {
                    crate::config::ignore::should_skip_for_lang_with_patterns(
                        name,
                        lang,
                        &extra_patterns,
                    )
                } else {
                    crate::config::ignore::should_skip_with_patterns(name, &extra_patterns)
                }
            };
            // Use ignore::WalkBuilder for .gitignore support
            let mut builder = ignore_crate::WalkBuilder::new(&self.path);
            builder
                .hidden(self.skip_hidden)
                .git_ignore(true)
                .git_exclude(true)
                .follow_links(self.follow_links);
            if let Some(depth) = self.max_depth {
                builder.max_depth(Some(depth));
            }
            let iter = builder
                .filter_entry(move |entry: &ignore_crate::DirEntry| {
                    let name = entry.file_name().to_string_lossy();
                    !predicate(&name)
                })
                .build()
                .map(|result| {
                    result
                        .map(Entry::Ignore)
                        .map_err(|e| anyhow!("Walk error: {}", e))
                });
            Box::new(iter)
        } else {
            let extra_patterns = crate::config::ignore::load_project_ignore_patterns(&self.path);
            let predicate = move |name: &str| {
                if skip_hidden && name.starts_with('.') {
                    return true;
                }
                if let Some(lang) = language {
                    crate::config::ignore::should_skip_for_lang_with_patterns(
                        name,
                        lang,
                        &extra_patterns,
                    )
                } else {
                    crate::config::ignore::should_skip_with_patterns(name, &extra_patterns)
                }
            };
            // Use walkdir::WalkDir for simple walking
            let mut walker = walkdir::WalkDir::new(&self.path).follow_links(self.follow_links);
            if let Some(depth) = self.max_depth {
                walker = walker.max_depth(depth);
            }
            let iter = walker
                .into_iter()
                .filter_entry(move |entry: &walkdir::DirEntry| {
                    let name = entry.file_name().to_string_lossy();
                    !predicate(&name)
                })
                .map(|result| {
                    result
                        .map(Entry::WalkDir)
                        .map_err(|e| anyhow!("Walk error: {}", e))
                });
            Box::new(iter)
        }
    }
}

/// Convenience function to walk Rust files with .gitignore support.
pub fn walk_rust_files(project_path: &str) -> Result<Vec<String>> {
    FileWalker::new(project_path)
        .for_language("rust")
        .walk_relative()
        .collect::<Result<Vec<_>>>()
        .context("Failed to walk Rust files")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    // ============================================================================
    // FileWalker builder tests
    // ============================================================================

    #[test]
    fn test_file_walker_new_defaults() {
        let walker = FileWalker::new("/test/path");
        assert_eq!(walker.path, PathBuf::from("/test/path"));
        assert!(walker.follow_links);
        assert!(walker.use_gitignore);
        assert!(walker.extensions.is_empty());
        assert!(walker.language.is_none());
        assert!(walker.skip_hidden);
        assert!(walker.max_depth.is_none());
    }

    #[test]
    fn test_file_walker_follow_links() {
        let walker = FileWalker::new("/test").follow_links(false);
        assert!(!walker.follow_links);
    }

    #[test]
    fn test_file_walker_use_gitignore() {
        let walker = FileWalker::new("/test").use_gitignore(false);
        assert!(!walker.use_gitignore);
    }

    #[test]
    fn test_file_walker_with_extension() {
        let walker = FileWalker::new("/test")
            .with_extension("rs")
            .with_extension("toml");
        assert_eq!(walker.extensions, vec!["rs", "toml"]);
    }

    #[test]
    fn test_file_walker_for_language_rust() {
        let walker = FileWalker::new("/test").for_language("rust");
        assert_eq!(walker.language, Some("rust"));
        assert!(walker.extensions.contains(&"rs"));
    }

    #[test]
    fn test_file_walker_for_language_python() {
        let walker = FileWalker::new("/test").for_language("python");
        assert_eq!(walker.language, Some("python"));
        assert!(walker.extensions.contains(&"py"));
    }

    #[test]
    fn test_file_walker_for_language_typescript() {
        let walker = FileWalker::new("/test").for_language("typescript");
        assert_eq!(walker.language, Some("typescript"));
        assert!(walker.extensions.contains(&"ts"));
    }

    #[test]
    fn test_file_walker_for_language_javascript() {
        let walker = FileWalker::new("/test").for_language("javascript");
        assert_eq!(walker.language, Some("javascript"));
        assert!(walker.extensions.contains(&"js"));
    }

    #[test]
    fn test_file_walker_for_language_go() {
        let walker = FileWalker::new("/test").for_language("go");
        assert_eq!(walker.language, Some("go"));
        assert!(walker.extensions.contains(&"go"));
    }

    #[test]
    fn test_file_walker_for_language_unknown() {
        let walker = FileWalker::new("/test").for_language("unknown");
        assert_eq!(walker.language, Some("unknown"));
        assert!(walker.extensions.is_empty());
    }

    #[test]
    fn test_file_walker_skip_hidden() {
        let walker = FileWalker::new("/test").skip_hidden(false);
        assert!(!walker.skip_hidden);
    }

    #[test]
    fn test_file_walker_max_depth() {
        let walker = FileWalker::new("/test").max_depth(5);
        assert_eq!(walker.max_depth, Some(5));
    }

    #[test]
    fn test_file_walker_builder_chain() {
        let walker = FileWalker::new("/project")
            .for_language("rust")
            .follow_links(false)
            .skip_hidden(true)
            .max_depth(10)
            .use_gitignore(true);

        assert_eq!(walker.language, Some("rust"));
        assert!(!walker.follow_links);
        assert!(walker.skip_hidden);
        assert_eq!(walker.max_depth, Some(10));
        assert!(walker.use_gitignore);
    }

    // ============================================================================
    // should_include_file tests
    // ============================================================================

    #[test]
    fn test_should_include_file_no_extensions() {
        let walker = FileWalker::new("/test");
        assert!(walker.should_include_file(Path::new("test.rs")));
        assert!(walker.should_include_file(Path::new("test.py")));
        assert!(walker.should_include_file(Path::new("no_extension")));
    }

    #[test]
    fn test_should_include_file_with_extensions() {
        let walker = FileWalker::new("/test").with_extension("rs");
        assert!(walker.should_include_file(Path::new("test.rs")));
        assert!(!walker.should_include_file(Path::new("test.py")));
        assert!(!walker.should_include_file(Path::new("no_extension")));
    }

    #[test]
    fn test_should_include_file_multiple_extensions() {
        let walker = FileWalker::new("/test")
            .with_extension("rs")
            .with_extension("toml");
        assert!(walker.should_include_file(Path::new("test.rs")));
        assert!(walker.should_include_file(Path::new("Cargo.toml")));
        assert!(!walker.should_include_file(Path::new("test.py")));
    }

    // ============================================================================
    // walk_paths integration tests
    // ============================================================================

    #[test]
    fn test_walk_paths_basic() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("test.rs"), "fn main() {}").unwrap();
        fs::write(dir.path().join("lib.rs"), "pub mod test;").unwrap();
        fs::write(dir.path().join("README.md"), "# Test").unwrap();

        let walker = FileWalker::new(dir.path()).for_language("rust");
        let paths: Vec<_> = walker.walk_paths().filter_map(|p| p.ok()).collect();

        assert_eq!(paths.len(), 2);
        assert!(paths.iter().any(|p| p.ends_with("test.rs")));
        assert!(paths.iter().any(|p| p.ends_with("lib.rs")));
    }

    #[test]
    fn test_walk_paths_respects_hidden() {
        let dir = TempDir::new().unwrap();
        // Create a non-hidden subdirectory to walk from (temp dir itself may be hidden)
        let project = dir.path().join("project");
        fs::create_dir(&project).unwrap();
        fs::write(project.join("visible.rs"), "").unwrap();
        fs::create_dir(project.join(".hidden")).unwrap();
        fs::write(project.join(".hidden/secret.rs"), "").unwrap();

        let walker = FileWalker::new(&project)
            .for_language("rust")
            .skip_hidden(true)
            .use_gitignore(false);
        let paths: Vec<_> = walker.walk_paths().filter_map(|p| p.ok()).collect();

        assert_eq!(paths.len(), 1);
        assert!(paths[0].ends_with("visible.rs"));
    }

    #[test]
    fn test_walk_paths_max_depth() {
        let dir = TempDir::new().unwrap();
        // Create a non-hidden subdirectory to walk from (temp dir itself may be hidden)
        let project = dir.path().join("project");
        fs::create_dir(&project).unwrap();
        fs::write(project.join("root.rs"), "").unwrap();
        fs::create_dir(project.join("level1")).unwrap();
        fs::write(project.join("level1/file1.rs"), "").unwrap();
        fs::create_dir(project.join("level1/level2")).unwrap();
        fs::write(project.join("level1/level2/file2.rs"), "").unwrap();

        let walker = FileWalker::new(&project)
            .for_language("rust")
            .max_depth(2)
            .use_gitignore(false);
        let paths: Vec<_> = walker.walk_paths().filter_map(|p| p.ok()).collect();

        // Should get root.rs and level1/file1.rs but not level1/level2/file2.rs
        assert_eq!(paths.len(), 2);
    }

    // ============================================================================
    // walk_relative tests
    // ============================================================================

    #[test]
    fn test_walk_relative() {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/main.rs"), "fn main() {}").unwrap();

        let walker = FileWalker::new(dir.path()).for_language("rust");
        let paths: Vec<_> = walker.walk_relative().filter_map(|p| p.ok()).collect();

        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0], "src/main.rs");
    }

    // ============================================================================
    // Entry tests
    // ============================================================================

    #[test]
    fn test_entry_path() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("test.rs");
        fs::write(&file_path, "").unwrap();

        let walker = FileWalker::new(dir.path()).for_language("rust");
        let entries: Vec<_> = walker.walk_entries().filter_map(|e| e.ok()).collect();

        // Should have at least the file
        assert!(entries.iter().any(|e| e.path().ends_with("test.rs")));
    }
}
