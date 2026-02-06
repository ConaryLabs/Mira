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
| `path_to_string()` | Convert a `Path` to `String` |
| `relative_to()` | Strip a path prefix to get a relative path |
| `truncate()` | Truncate a string with ellipsis at a given length |
