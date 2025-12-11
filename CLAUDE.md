# CLAUDE.md

This file provides guidance to Claude Code when working with code in this repository.

## Project Overview

Mira is a **Power Suit for Claude Code** - a memory and intelligence layer that provides persistent storage, semantic search, and cross-session context through the Model Context Protocol (MCP).

**Key concept**: Claude Code drives all AI interactions. Mira provides storage and intelligence capabilities that Claude Code can't do natively:
- Persistent memory across sessions with semantic search
- Cross-session context (what did we work on before?)
- Task tracking that persists
- Code intelligence (symbols, call graphs)
- Git intelligence (cochange patterns, error fixes)

## Repository Structure

```
mira/
├── src/
│   ├── main.rs       # MCP server entry point (348 lines)
│   ├── lib.rs        # Library exports (4 lines)
│   └── tools/        # MCP tool implementations
│       ├── mod.rs        # Module exports
│       ├── types.rs      # Request types for all tools
│       ├── response.rs   # Response helpers (json_response, etc.)
│       ├── semantic.rs   # SemanticSearch (Qdrant + OpenAI)
│       ├── memory.rs     # remember, recall, forget
│       ├── sessions.rs   # store_session, search_sessions
│       ├── tasks.rs      # task management
│       ├── code_intel.rs # symbols, call graph, semantic search
│       ├── git_intel.rs  # commits, cochange, error fixes
│       ├── build_intel.rs# build tracking
│       ├── documents.rs  # document search
│       ├── workspace.rs  # activity/context tracking
│       ├── project.rs    # guidelines
│       └── analytics.rs  # list_tables, query
├── migrations/       # Database schema
├── data/             # SQLite database (runtime, gitignored)
├── Cargo.toml        # Rust dependencies
└── .sqlx/            # SQLx offline mode cache
```

## Development Commands

```bash
# Build
SQLX_OFFLINE=true cargo build --release

# Run (as MCP server with semantic search)
DATABASE_URL="sqlite://data/mira.db" \
QDRANT_URL="http://localhost:6334" \
OPENAI_API_KEY="sk-..." \
./target/release/mira

# Linting
SQLX_OFFLINE=true cargo clippy
cargo fmt
```

## Architecture

### MCP Server

Single binary MCP server communicating via stdio JSON-RPC:

```
Claude Code  <--MCP(stdio)-->  Mira Server
                                   |
                     +-------------+-------------+
                     |             |             |
                  SQLite        Qdrant      Direct SQL
                 (facts,       (vectors)    (all queries)
                 sessions,     semantic
                 tasks)        search)
```

### Semantic Search

When Qdrant and OpenAI are configured:
- **Model**: text-embedding-3-large (3072 dimensions)
- **Collections**: mira_code, mira_conversation, mira_docs
- **Fallback**: Text search when Qdrant/OpenAI unavailable

### 36 MCP Tools

**Memory** (semantic search):
- `remember` - Store facts, decisions, preferences
- `recall` - Search memories by meaning
- `forget` - Delete a memory

**Cross-Session Context**:
- `store_session` - Store session summary
- `search_sessions` - Find past sessions
- `store_decision` - Record decisions

**Tasks**:
- `create_task`, `list_tasks`, `get_task`, `update_task`, `complete_task`, `delete_task`

**Code Intelligence**:
- `get_symbols` - File symbols
- `get_call_graph` - Call relationships
- `get_related_files` - Related files
- `semantic_code_search` - Natural language code search

**Git Intelligence**:
- `get_recent_commits`, `search_commits`
- `find_cochange_patterns` - Files that change together
- `find_similar_fixes` - Past error fixes
- `record_error_fix` - Record a fix

**Build Intelligence**:
- `record_build`, `record_build_error`, `get_build_errors`, `resolve_error`

**Documents**:
- `list_documents`, `search_documents`, `get_document`

**Workspace**:
- `record_activity`, `get_recent_activity`, `set_context`, `get_context`

**Project**:
- `get_guidelines`, `add_guideline`

**Analytics**:
- `list_tables`, `query`

## Configuration

### Claude Code MCP Config

Add to `~/.claude/mcp.json`:

```json
{
  "mcpServers": {
    "mira": {
      "command": "/path/to/mira/target/release/mira",
      "env": {
        "DATABASE_URL": "sqlite:///path/to/mira/data/mira.db",
        "QDRANT_URL": "http://localhost:6334",
        "OPENAI_API_KEY": "sk-..."
      }
    }
  }
}
```

### Environment Variables

| Variable | Description | Required |
|----------|-------------|----------|
| `DATABASE_URL` | SQLite connection | Yes |
| `QDRANT_URL` | Qdrant gRPC endpoint | No (semantic search disabled) |
| `OPENAI_API_KEY` | OpenAI embeddings | No (semantic search disabled) |
| `RUST_LOG` | Log level | No (default: info) |

## Prerequisites

- **Rust 1.91+** with edition 2024
- **SQLite 3.35+**
- **Qdrant 1.16+** (optional)
- **OpenAI API key** (optional)

## Code Style

- `cargo fmt` before committing
- `cargo clippy` and fix warnings
- No emojis in code/comments
