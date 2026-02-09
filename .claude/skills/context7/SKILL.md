---
name: context7
description: Look up external library documentation using Context7 to get current API references instead of guessing from training data.
---

# Context7: External Library Documentation

Use Context7 to look up current library APIs instead of guessing from training data.

## When to Use

1. **Implementing with external libraries** - Check current API before writing code
2. **Debugging library errors** - Verify correct usage patterns
3. **User asks "how do I use X"** - Get up-to-date examples
4. **Uncertain about library API** - Don't guess, look it up
5. **Library version matters** - Context7 has version-specific docs

## Workflow

```
resolve-library-id(libraryName="tokio", query="async runtime spawn tasks")
query-docs(libraryId="/tokio-rs/tokio", query="how to spawn async tasks")
```

Always call `resolve-library-id` first to get the correct library ID, then `query-docs` with a specific question.

## When NOT to Use

- Standard library features (Rust std, Python builtins, etc.)
- Confident in the API from recent experience
- Simple operations with well-known patterns

## Example

User asks about async file reading with tokio:
1. `resolve-library-id(libraryName="tokio", query="async file reading")`
2. `query-docs(libraryId="/tokio-rs/tokio", query="async file reading tokio::fs")`
3. Use the returned docs to write correct code
