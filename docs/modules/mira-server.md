# mira-server

The core server crate providing memory, code intelligence, and persistent context for AI agents through an MCP (Model Context Protocol) interface.

## Architecture

The crate follows a modular design with clear separation between protocol handling, data persistence, and intelligence features.

**Entry point:** `main.rs` implements a CLI with subcommands (`serve`, `tool`, `index`, `hook`, `debug-*`).

## Modules

| Module | Purpose |
|--------|---------|
| `mcp` | MCP protocol server implementation |
| `tools` | MCP tool handlers (remember, recall, search, etc.) |
| `db` | Database layer with SQLite + sqlite-vec |
| `llm` | LLM provider abstraction (DeepSeek, Gemini) |
| `indexer` | Code indexing and symbol extraction |
| `cartographer` | Codebase mapping and module detection |
| `search` | Semantic code search and memory recall |
| `embeddings` | Vector embedding generation |
| `background` | Background workers (embeddings, summaries, health checks) |
| `experts` | Expert consultation system |
| `hooks` | Claude Code lifecycle hooks |
| `proactive` | Proactive analysis and suggestions |
| `config` | Configuration management |
| `context` | Session context and state |
| `cross_project` | Cross-project pattern sharing |
| `identity` | User and team identity |
| `git` | Git repository analysis |
| `http` | HTTP client utilities |
| `project_files` | Project file discovery and filtering |
| `proxy` | MCP proxy/routing |
| `cli` | CLI subcommands |
| `error` | Error types (`MiraError`) |
| `utils` | Shared utilities |

## Key Exports

- `MiraError` - Main error type
- `Result<T>` - Type alias for `Result<T, MiraError>`
