# error

Standardized error types using thiserror.

## Key Type

`MiraError` - Main error enum with variants:

| Variant | Description |
|---------|-------------|
| `InvalidInput` | Invalid user input |
| `ProjectNotSet` | No active project context |
| `Db` | Database errors (rusqlite) |
| `Io` | I/O errors |
| `Json` | JSON serialization errors |
| `Http` | HTTP client errors |
| `Git` | Git operation errors |
| `TreeSitter` | Tree-sitter parsing errors |
| `Embedding` | Embedding generation errors |
| `Llm` | LLM provider errors |
| `Cancelled` | Operation cancelled |
| `Config` | Configuration errors |
| `Anyhow` | Wrapped anyhow errors |
| `Other` | Catch-all for other errors |

## Export

`Result<T>` - Type alias for `Result<T, MiraError>`.
