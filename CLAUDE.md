# CLAUDE.md

This file provides guidance to Claude Code when working with code in this repository.

## Project Overview

Mira is a **Power Suit for Claude Code** - a memory and intelligence layer that provides persistent storage, code intelligence, git intelligence, and project context through the Model Context Protocol (MCP).

**Key concept**: Claude Code drives all AI interactions. Mira provides storage and intelligence capabilities that Claude Code can't do natively:
- Persistent memory across sessions
- Code analysis and symbol tracking
- Git expertise and co-change patterns
- Project guidelines and conventions

## Repository Structure

```
mira/
└── backend/          # Rust MCP server (single binary)
    ├── src/
    │   ├── main.rs           # MCP server with 22 tools
    │   ├── lib.rs            # Library exports
    │   ├── llm.rs            # Embedding provider (OpenAI)
    │   ├── memory/           # Memory storage (SQLite + Qdrant)
    │   ├── git/              # Git intelligence
    │   ├── build/            # Build error tracking
    │   ├── watcher/          # File system watching
    │   └── config/           # Configuration
    ├── data/                 # SQLite database
    └── migrations/           # Database migrations
```

## Development Commands

```bash
cd backend

# Build
cargo build                    # Debug build
cargo build --release          # Release build

# Run (as MCP server)
DATABASE_URL="sqlite://data/mira.db" ./target/release/mira

# Run tests
cargo test

# Linting
cargo clippy
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
                 sessions)                   cochange)
```

### 22 MCP Tools

**Memory Tools**:
- `remember` - Store facts, decisions, preferences
- `recall` - Search stored memories
- `forget` - Remove a memory

**Code Intelligence**:
- `get_symbols` - Get symbols from a file
- `get_call_graph` - Function call relationships
- `get_related_files` - Related files by imports/cochange

**Git Intelligence**:
- `get_file_experts` - Find developers with expertise
- `find_similar_fixes` - Search past error fixes
- `get_change_risk` - Assess change risk
- `find_cochange_patterns` - Files that change together

**Project Context**:
- `get_guidelines` - Get coding guidelines
- `add_guideline` - Add a guideline

**Session & Data**:
- `list_sessions`, `get_session`, `search_memories`, `get_recent_messages`
- `list_operations`, `get_budget_status`, `get_cache_stats`, `get_tool_usage`
- `list_tables`, `query`

### Database Schema

Key tables:
- `memory_facts` - Stored memories (remember/recall)
- `project_guidelines` - Project conventions
- `code_elements` - Parsed code symbols
- `call_graph` - Function relationships
- `semantic_nodes/edges` - Code semantic graph
- `file_cochange_patterns` - Git co-change analysis
- `author_expertise` - Developer expertise
- `historical_fixes` - Past error fixes

## Configuration

### Claude Code MCP Config

Add to `~/.claude/mcp.json`:

```json
{
  "mcpServers": {
    "mira": {
      "command": "/home/peter/Mira/backend/target/release/mira",
      "env": {
        "DATABASE_URL": "sqlite:///home/peter/Mira/backend/data/mira.db"
      }
    }
  }
}
```

### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `DATABASE_URL` | SQLite connection string | `sqlite://data/mira.db` |
| `QDRANT_URL` | Qdrant gRPC endpoint | `http://localhost:6334` |
| `OPENAI_API_KEY` | For embeddings (optional) | - |

## Prerequisites

- **Rust 1.91+** (target version)
- **Rust Edition 2024**
- **SQLite 3.35+**
- **Qdrant 1.16+** (optional, for semantic search)

## Code Style

- Run `cargo fmt` before committing
- Run `cargo clippy` and address warnings
- File headers: `// backend/src/path/to/file.rs`
- No emojis in code or comments
- Keep comments concise

## Key Files

- `src/main.rs` - MCP server with all 22 tools
- `src/lib.rs` - Library exports
- `src/llm.rs` - Embedding provider (OpenAI text-embedding-3-large)
- `src/memory/` - Memory storage system
- `src/memory/context.rs` - Context types for recall operations
- `src/git/` - Git intelligence
- `src/git/error.rs` - Git error types

## Testing

```bash
# Run all tests
cargo test

# Test MCP server manually
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}' | DATABASE_URL="sqlite://data/mira.db" ./target/release/mira

# Test tools/list
python3 -c "
import subprocess, json
proc = subprocess.Popen(['./target/release/mira'], stdin=subprocess.PIPE, stdout=subprocess.PIPE, env={'DATABASE_URL': 'sqlite://data/mira.db'}, text=True)
proc.stdin.write(json.dumps({'jsonrpc':'2.0','id':1,'method':'initialize','params':{'protocolVersion':'2024-11-05','capabilities':{},'clientInfo':{'name':'test','version':'1.0'}}}) + '\n')
proc.stdin.flush()
print(proc.stdout.readline())
proc.terminate()
"
```
