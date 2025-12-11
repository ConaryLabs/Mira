# CLAUDE.md

This file provides guidance to Claude Code when working with code in this repository.

## Project Overview

Mira is a **Power Suit for Claude Code** - a memory and intelligence layer that provides persistent storage, semantic search, code intelligence, git intelligence, and cross-session context through the Model Context Protocol (MCP).

**Key concept**: Claude Code drives all AI interactions. Mira provides storage and intelligence capabilities that Claude Code can't do natively:
- Persistent memory across sessions with semantic search
- Cross-session context (what did we work on before?)
- Code analysis and symbol tracking
- Git expertise and co-change patterns
- Project guidelines and conventions

## Repository Structure

```
mira/
└── backend/          # Rust MCP server (single binary)
    ├── src/
    │   ├── main.rs           # MCP server with 36 tools
    │   ├── lib.rs            # Library exports
    │   ├── tools/            # MCP tool implementations
    │   │   ├── mod.rs        # Module exports
    │   │   ├── types.rs      # Request types for all tools
    │   │   ├── analytics.rs  # list_tables, query
    │   │   ├── memory.rs     # remember, recall, forget (semantic)
    │   │   ├── sessions.rs   # store_session, search_sessions, store_decision
    │   │   ├── semantic.rs   # SemanticSearch (Qdrant + OpenAI)
    │   │   ├── tasks.rs      # task management
    │   │   ├── code_intel.rs # code intelligence + semantic_code_search
    │   │   ├── git_intel.rs  # git intelligence + semantic fixes
    │   │   ├── build_intel.rs# build tracking
    │   │   ├── workspace.rs  # activity/context tracking
    │   │   ├── documents.rs  # document search (semantic)
    │   │   └── project.rs    # guidelines
    │   ├── llm.rs            # Embedding provider (OpenAI)
    │   ├── memory/           # Memory storage (legacy)
    │   ├── git/              # Git intelligence
    │   └── config/           # Configuration
    ├── data/                 # SQLite database + Qdrant storage
    └── migrations/           # Database migrations
```

## Development Commands

```bash
cd backend

# Build (use SQLX_OFFLINE to bypass compile-time SQL checks)
SQLX_OFFLINE=true cargo build                    # Debug build
SQLX_OFFLINE=true cargo build --release          # Release build

# Run (as MCP server with semantic search)
DATABASE_URL="sqlite://data/mira.db" \
QDRANT_URL="http://localhost:6334" \
OPENAI_API_KEY="sk-..." \
./target/release/mira

# Start Qdrant (if not running)
./bin/qdrant &

# Database setup
DATABASE_URL="sqlite://data/mira.db" sqlx database create
DATABASE_URL="sqlite://data/mira.db" sqlx migrate run

# Run tests
SQLX_OFFLINE=true cargo test

# Linting
SQLX_OFFLINE=true cargo clippy
cargo fmt
```

## Architecture

### MCP Server

Mira runs as a single binary MCP server that communicates via stdio JSON-RPC:

```
Claude Code  <--MCP(stdio)-->  Mira Server
                                   |
                     +-------------+-------------+
                     |             |             |
                  SQLite        Qdrant      Git Analysis
                 (facts,       (vectors)    (expertise,
                 sessions,     semantic      cochange)
                 tasks)        search)
```

### Semantic Search

When Qdrant and OpenAI are configured, Mira provides semantic similarity search:
- **Model**: text-embedding-3-large (3072 dimensions)
- **Collections**: mira_code, mira_conversation, mira_docs
- **Fallback**: Text search when Qdrant/OpenAI unavailable

### 36 MCP Tools

**Memory Tools** (with semantic search):
- `remember` - Store facts, decisions, preferences (stored in SQLite + Qdrant)
- `recall` - Search memories by semantic similarity
- `forget` - Remove a memory by ID

**Cross-Session Context** (semantic search):
- `store_session` - Store session summary for future reference
- `search_sessions` - Find relevant past sessions by meaning
- `store_decision` - Record important decisions with context

**Task Management**:
- `create_task` - Create a persistent task
- `list_tasks` - List tasks by status/project/parent
- `get_task` - Get task details
- `update_task` - Update task title/description/status/priority
- `complete_task` - Mark task as completed with notes
- `delete_task` - Delete a task

**Code Intelligence** (with semantic search):
- `get_symbols` - Get symbols from a file
- `get_call_graph` - Function call relationships
- `get_related_files` - Related files by imports/cochange
- `semantic_code_search` - Find code by natural language description

**Git Intelligence** (with semantic search):
- `get_recent_commits` - Get recent git commits
- `search_commits` - Search commit messages
- `find_cochange_patterns` - Files that change together
- `find_similar_fixes` - Search past error fixes by meaning
- `record_error_fix` - Record an error->fix pattern

**Build Intelligence**:
- `record_build` - Record a build run
- `record_build_error` - Record a build error
- `get_build_errors` - Get unresolved build errors
- `resolve_error` - Mark an error as resolved

**Document Search** (with semantic search):
- `list_documents` - List stored documents
- `search_documents` - Search documents by semantic similarity
- `get_document` - Get document details

**Workspace Context**:
- `record_activity` - Record file activity (read/write/error/test)
- `get_recent_activity` - Get recent file activity
- `set_context` - Set work context with optional TTL
- `get_context` - Get active work context

**Project Context**:
- `get_guidelines` - Get coding guidelines
- `add_guideline` - Add a guideline

**Analytics**:
- `list_tables` - List all tables with row counts
- `query` - Execute read-only SQL queries

### Database Schema

Core tables:
- `memory_facts` - Stored memories with key/value/type/category
- `coding_guidelines` - Project coding conventions
- `tasks` - Persistent tasks with UUID IDs
- `code_symbols` - Parsed code symbols (function, class, struct, etc.)
- `call_graph` - Function call relationships
- `imports` - Import/dependency tracking
- `cochange_patterns` - Files that change together (git analysis)
- `error_fixes` - Historical error->fix patterns
- `git_commits` - Recent git commit history
- `build_runs` - Build execution history
- `build_errors` - Build errors for tracking/learning
- `documents` - Uploaded/indexed documents for RAG
- `document_chunks` - Document chunks for semantic search
- `file_activity` - Recent file activity tracking
- `work_context` - Active work context with TTL

## Configuration

### Claude Code MCP Config

Add to `~/.claude/mcp.json`:

```json
{
  "mcpServers": {
    "mira": {
      "command": "/home/peter/Mira/backend/target/release/mira",
      "env": {
        "DATABASE_URL": "sqlite:///home/peter/Mira/backend/data/mira.db",
        "QDRANT_URL": "http://localhost:6334",
        "OPENAI_API_KEY": "sk-..."
      }
    }
  }
}
```

### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `DATABASE_URL` | SQLite connection string | `sqlite://data/mira.db` |
| `QDRANT_URL` | Qdrant gRPC endpoint (optional) | - |
| `OPENAI_API_KEY` | For embeddings (optional) | - |

**Note**: Semantic search requires both `QDRANT_URL` and `OPENAI_API_KEY`. Without them, Mira falls back to text-based search.

## Prerequisites

- **Rust 1.91+** (target version)
- **Rust Edition 2024**
- **SQLite 3.35+**
- **Qdrant 1.16+** (optional, for semantic search)
- **OpenAI API key** (optional, for embeddings)

## Code Style

- Run `cargo fmt` before committing
- Run `cargo clippy` and address warnings
- File headers: `// backend/src/path/to/file.rs`
- No emojis in code or comments
- Keep comments concise

## Key Files

- `src/main.rs` - MCP server with all 36 tools
- `src/tools/` - Tool implementations
- `src/tools/semantic.rs` - SemanticSearch client (Qdrant + OpenAI)
- `src/tools/sessions.rs` - Cross-session memory tools
- `src/tools/types.rs` - Request types for all tools
- `src/lib.rs` - Library exports
- `src/llm.rs` - Embedding provider
- `migrations/20251211000001_fresh_schema.sql` - Database schema

## Testing

```bash
# Run all tests
cargo test

# Test MCP server manually (with semantic search)
export DATABASE_URL="sqlite://data/mira.db"
export QDRANT_URL="http://localhost:6334"
export OPENAI_API_KEY="sk-..."

# Initialize and test remember
(printf '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}\n' && sleep 0.5 && printf '{"jsonrpc":"2.0","method":"notifications/initialized"}\n' && sleep 0.5 && printf '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"remember","arguments":{"content":"Test memory","fact_type":"test"}}}\n' && sleep 2) | ./target/release/mira

# Test semantic recall
(printf '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}\n' && sleep 0.5 && printf '{"jsonrpc":"2.0","method":"notifications/initialized"}\n' && sleep 0.5 && printf '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"recall","arguments":{"query":"what was stored?"}}}\n' && sleep 2) | ./target/release/mira
```
