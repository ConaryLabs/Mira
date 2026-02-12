<!-- docs/modules/mira-server.md -->
# mira-server

The core server crate providing memory, code intelligence, and persistent context for AI agents through an MCP (Model Context Protocol) interface.

## Overview

Mira is a Rust MCP server that gives Claude Code persistent memory, semantic code search, and proactive context injection. It runs as a stdio-based MCP server spawned by Claude Code, backed by SQLite with sqlite-vec for vector operations.

## Architecture

The crate follows a layered design: protocol handling (`mcp/`), tool implementations (`tools/`), data persistence (`db/`), and intelligence features (`search/`, `context/`, `llm/`).

**Entry point:** `main.rs` implements a CLI with subcommands: `serve` (default, MCP stdio), `tool` (single tool invocation), `index` (code indexing), `hook` (Claude Code lifecycle hooks), `debug-carto`/`debug-session` (diagnostics), `config`, `setup`, `status-line`.

## Modules

| Module | Purpose |
|--------|---------|
| `config` | Configuration from env vars and `~/.mira/config.toml` |
| `db` | SQLite database layer with connection pooling and migrations |
| `mcp` | MCP protocol server (router, handler, requests, responses) |
| `tools` | MCP tool handler implementations |
| `hooks` | Claude Code lifecycle hooks (session, prompt, tool events) |
| `search` | Semantic, keyword, and cross-reference code search |
| `context` | Proactive context injection with budget management |
| `llm` | LLM provider abstraction (DeepSeek, Zhipu, Ollama, MCP Sampling) |
| `embeddings` | OpenAI embedding client for vector operations |
| `indexer` | Code indexing and symbol extraction via tree-sitter |
| `cartographer` | Codebase mapping and module detection |
| `background` | Background workers (embeddings, summaries, health checks) |
| `entities` | Heuristic entity extraction for memory recall boosting |
| `fuzzy` | Nucleo-based fuzzy fallback when embeddings unavailable |
| `git` | Git operations (branch detection, commits, diffs) |
| `identity` | User identity detection for multi-user memory scoping |
| `proactive` | Proactive analysis and suggestions |
| `error` | Error types (`MiraError` via thiserror) |
| `utils` | Shared utilities (path handling, JSON parsing, truncation) |
| `http` | HTTP client utilities |
| `tasks` | Async task management |
| `project_files` | Project file discovery and filtering |

## Key Exports

- `MiraError` -- Main error type
- `Result<T>` -- Type alias for `Result<T, MiraError>`
