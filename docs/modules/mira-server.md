# mira-server

The core server crate providing memory, code intelligence, and persistent context for AI agents through an MCP (Model Context Protocol) interface.

## Architecture

The crate follows a modular design with clear separation between protocol handling, data persistence, and intelligence features.

**Entry point:** `main.rs` implements a CLI with subcommands (`serve`, `tool`, `index`, `hook`, `debug-*`).

## Modules

| Module | Purpose |
|--------|---------|
| `background` | Background workers (embeddings, summaries, health checks) |
| `cartographer` | Codebase mapping and module detection |
| `cli` | CLI subcommands (binary entrypoint) |
| `config` | Configuration management |
| `context` | Session context and state |
| `db` | Database layer with SQLite + sqlite-vec |
| `embeddings` | OpenAI embedding client (`EmbeddingClient`) |
| `entities` | Entity extraction and linking |
| `error` | Error types (`MiraError`) |
| `fuzzy` | Fuzzy search fallback cache |
| `git` | Git operations (branch via git2, commits/diffs via CLI) |
| `hooks` | Claude Code lifecycle hooks |
| `http` | HTTP client utilities |
| `identity` | User and team identity |
| `indexer` | Code indexing and symbol extraction |
| `llm` | LLM provider abstraction (DeepSeek, Zhipu, Ollama, MCP Sampling) |
| `mcp` | MCP protocol server (includes router, client, elicitation, tasks) |
| `project_files` | Project file discovery and filtering |
| `proactive` | Proactive analysis and suggestions |
| `search` | Semantic code search and memory recall |
| `tasks` | Async task management |
| `tools` | MCP tool handlers (remember, recall, search, etc.) |
| `utils` | Shared utilities (JSON helpers) |

## Key Exports

- `MiraError` - Main error type
- `Result<T>` - Type alias for `Result<T, MiraError>`
