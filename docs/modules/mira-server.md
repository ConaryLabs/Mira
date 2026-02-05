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
| `cross_project` | Cross-project pattern sharing |
| `db` | Database layer with SQLite + sqlite-vec |
| `elicitation` | Elicitation (interactive user input) support |
| `embeddings` | Vector embedding generation |
| `entities` | Entity extraction and linking |
| `error` | Error types (`MiraError`) |
| `fuzzy` | Fuzzy search fallback cache |
| `git` | Git repository analysis |
| `hooks` | Claude Code lifecycle hooks |
| `http` | HTTP client utilities |
| `identity` | User and team identity |
| `indexer` | Code indexing and symbol extraction |
| `llm` | LLM provider abstraction (DeepSeek, Gemini) |
| `mcp` | MCP protocol server implementation |
| `mcp_client` | MCP client for accessing host environment tools from experts |
| `project_files` | Project file discovery and filtering |
| `proactive` | Proactive analysis and suggestions |
| `search` | Semantic code search and memory recall |
| `tools` | MCP tool handlers (remember, recall, search, etc.) |
| `utils` | Shared utilities |

## Key Exports

- `MiraError` - Main error type
- `Result<T>` - Type alias for `Result<T, MiraError>`
