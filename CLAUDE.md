# CLAUDE.md

This project uses **Mira** - a persistent memory and code intelligence layer via MCP.

## Session Start

Call once at the start of every session:
```
session_start(project_path="/home/peter/Mira")
```

## Available Tools

| Tool | Description |
|------|-------------|
| `session_start` | Initialize session with project |
| `remember` | Store facts, decisions, preferences |
| `recall` | Search memories semantically |
| `get_symbols` | Get symbols from a file |
| `semantic_code_search` | Find code by meaning |
| `index` | Index project code |
| `task` | Manage tasks |
| `goal` | Manage goals |

## Build

```bash
cargo build --release
```

## Architecture

- `src/db.rs` - SQLite + sqlite-vec database
- `src/embeddings.rs` - Gemini embeddings API
- `src/mcp/` - MCP server and tools
- `src/indexer/` - Tree-sitter code parsing
- `src/hooks/` - Claude Code hooks
