# utils

Shared utility functions used across the codebase. Organized as a directory with sub-modules.

## Sub-modules

| Module | Purpose |
|--------|---------|
| `json` | JSON utility helpers |

## Key Exports

| Function/Trait | Purpose |
|---------------|---------|
| `ResultExt` | Trait adding `.str_err()` for `Result<T, E>` → `Result<T, String>` conversion |
| `path_to_string()` | Convert a `Path` to `String` (normalizes backslashes to forward slashes) |
| `relative_to()` | Strip a path prefix to get a relative path |
| `truncate()` | Truncate a string with ellipsis at a given length |
| `truncate_at_boundary()` | Truncate a `&str` at a UTF-8 char boundary (zero-alloc) |
| `sanitize_project_path()` | Replace path separators with `-` for directory names (cross-platform) |
| `format_period()` | Format `Option<u32>` days into human-readable period string |
