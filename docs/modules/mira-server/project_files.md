<!-- docs/modules/mira-server/project_files.md -->
# project_files

Unified file walking with configurable filtering and .gitignore support.

## Overview

Provides the `FileWalker` builder for walking project files with consistent filtering across the codebase. Supports both `.gitignore`-aware walking (via the `ignore` crate's `WalkBuilder`) and simple directory walking (via `walkdir`). Used by the indexer, cartographer, and code health scanner.

## Key Types

- `FileWalker` - Builder for configuring file walks (path, language, extensions, depth, gitignore)
- `Entry` - Unified entry type wrapping both `ignore::DirEntry` and `walkdir::DirEntry`

## Key Functions

- `FileWalker::new()` - Create walker for a path
- `FileWalker::for_language()` - Set language with default extensions (rust -> "rs", python -> "py", etc.)
- `FileWalker::walk_paths()` - Iterate absolute file paths
- `FileWalker::walk_relative()` - Iterate relative path strings
- `FileWalker::walk_entries()` - Iterate unified Entry types (files and directories)
- `walk_rust_files()` - Convenience function for Rust file walking

## Builder Options

| Method | Default | Purpose |
|--------|---------|---------|
| `follow_links()` | true | Follow symbolic links |
| `use_gitignore()` | true | Respect .gitignore files |
| `with_extension()` | none | Filter by file extension |
| `for_language()` | none | Set language with default extensions |
| `skip_hidden()` | true | Skip hidden files/directories |
| `max_depth()` | unlimited | Maximum traversal depth |

## Architecture Notes

When `.gitignore` support is enabled, uses the `ignore` crate which handles nested `.gitignore` files and `.git/info/exclude`. Additional skip patterns are loaded from project-level configuration via `crate::config::ignore`. Language-specific directory skipping filters out directories like `node_modules`, `__pycache__`, and `target/`.
