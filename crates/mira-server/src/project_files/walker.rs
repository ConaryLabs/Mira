//! Unified file walking with configurable filtering and .gitignore support.
//!
//! This module provides a `FileWalker` builder for walking project files with
//! consistent filtering across the codebase. Supports both `.gitignore`-aware
//! walking (via `ignore::WalkBuilder`) and simple directory walking (via
//! `walkdir::WalkDir`).

// crates/mira-server/src/project_files/walker.rs
use ::ignore as ignore_crate;
use walkdir;
use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};

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

    pub fn file_type(&self) -> std::fs::FileType {
        match self {
            Entry::Ignore(e) => e.file_type().unwrap(),
            Entry::WalkDir(e) => e.file_type(),
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

            // Skip directories
            if !entry.file_type().is_file() {
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
            let predicate = move |name: &str| {
                if skip_hidden && name.starts_with('.') {
                    return true;
                }
                if let Some(lang) = language {
                    crate::config::ignore::should_skip_for_lang(name, lang)
                } else {
                    crate::config::ignore::should_skip(name)
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
            let predicate = move |name: &str| {
                if skip_hidden && name.starts_with('.') {
                    return true;
                }
                if let Some(lang) = language {
                    crate::config::ignore::should_skip_for_lang(name, lang)
                } else {
                    crate::config::ignore::should_skip(name)
                }
            };
            // Use walkdir::WalkDir for simple walking
            let mut walker = walkdir::WalkDir::new(&self.path)
                .follow_links(self.follow_links);
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

