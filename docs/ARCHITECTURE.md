# Mira Architecture

**Version**: 3.1.0
**Last Updated**: 2026-01-01

## Overview

Mira is an MCP (Model Context Protocol) server that provides persistent memory and code intelligence for Claude Code. It uses rusqlite with sqlite-vec for all storage, eliminating external dependencies.

## Source Structure

```
src/
├── main.rs           # CLI entry point (serve, index, hook)
├── lib.rs            # Library exports
├── db.rs             # Database (rusqlite + sqlite-vec)
├── embeddings.rs     # OpenAI embeddings API client
├── background/       # Background worker for batch processing
│   ├── mod.rs        # Worker loop, spawns on service start
│   ├── scanner.rs    # Finds pending work items
│   ├── embeddings.rs # OpenAI Batch API (50% cheaper)
│   └── summaries.rs  # Rate-limited DeepSeek summaries
├── cartographer/     # Codebase structure mapping
│   └── mod.rs        # Module detection, dependency graphs
├── mcp/
│   ├── mod.rs        # MCP server (rmcp)
│   └── tools/        # Tool implementations
│       ├── mod.rs
│       ├── project.rs   # session_start, set_project, get_project
│       ├── memory.rs    # remember, recall, forget
│       ├── code.rs      # get_symbols, semantic_code_search, index, summarize_codebase
│       └── tasks.rs     # task, goal
├── indexer/
│   ├── mod.rs        # Code indexing orchestration
│   └── parsers/      # Tree-sitter parsers
│       ├── mod.rs
│       ├── rust.rs
│       ├── python.rs
│       ├── typescript.rs
│       └── go.rs
└── hooks/
    ├── mod.rs
    └── permission.rs # Auto-approval hook
```

## Data Flow

```
Claude Code
    │
    ▼ (stdio)
┌─────────────────────────────────────┐
│  MCP Server (rmcp)                  │
│  - Parses JSON-RPC                  │
│  - Routes to tool handlers          │
└─────────────────────────────────────┘
    │
    ▼
┌─────────────────────────────────────┐
│  Tool Implementations               │
│  - session_start, remember, recall  │
│  - get_symbols, semantic_code_search│
│  - task, goal                       │
└─────────────────────────────────────┘
    │
    ├─────────────┬─────────────┐
    ▼             ▼             ▼
┌─────────┐ ┌──────────┐ ┌───────────┐
│ SQLite  │ │sqlite-vec│ │  Gemini   │
│ Tables  │ │ Vectors  │ │ Embeddings│
└─────────┘ └──────────┘ └───────────┘
```

## Database Schema

### Regular Tables (15)

```sql
-- Projects
projects (id, path, name, created_at)

-- Memory
memory_facts (id, project_id, key, content, fact_type, category, confidence, created_at, updated_at)
corrections (id, project_id, what_was_wrong, what_is_right, correction_type, scope, confidence, created_at)

-- Code Intelligence
code_symbols (id, project_id, file_path, name, symbol_type, start_line, end_line, signature, indexed_at)
call_graph (id, caller_id, callee_name, callee_id, call_count)
imports (id, project_id, file_path, import_path, is_external)
codebase_modules (id, project_id, module_id, name, path, purpose, exports, dependencies, line_count)

-- Sessions
sessions (id, project_id, status, summary, started_at, last_activity)
tool_history (id, session_id, tool_name, arguments, result_summary, success, created_at)

-- Tasks & Goals
goals (id, project_id, title, description, status, priority, progress_percent, created_at)
milestones (id, goal_id, title, weight, completed, completed_at)
tasks (id, project_id, goal_id, title, description, status, priority, created_at)

-- Permissions
permission_rules (id, tool_name, pattern, match_type, scope, created_at)

-- Background Processing
pending_embeddings (id, project_id, file_path, chunk_content, status, created_at)
background_batches (id, batch_id, item_ids, status, created_at)
```

### Vector Tables (2)

```sql
-- sqlite-vec virtual tables with 3072-dimension OpenAI embeddings
vec_memory (embedding, fact_id, content)
vec_code (embedding, file_path, chunk_content, project_id)
```

## MCP Tools

| Tool | Description |
|------|-------------|
| `session_start` | Initialize session with project context |
| `set_project` | Set active project |
| `get_project` | Get current project |
| `remember` | Store a memory fact with optional embedding |
| `recall` | Semantic search through memories |
| `forget` | Delete a memory by ID |
| `get_symbols` | Get symbols from a file (via tree-sitter) |
| `semantic_code_search` | Search code by meaning |
| `index` | Index project code |
| `task` | Manage tasks (CRUD) |
| `goal` | Manage goals and milestones |

## Key Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| `rmcp` | 0.12 | MCP server framework |
| `rusqlite` | 0.32 | SQLite database |
| `sqlite-vec` | 0.1.6 | Vector embeddings extension |
| `tree-sitter` | 0.24 | Code parsing |
| `reqwest` | 0.12 | HTTP client (Gemini API) |
| `tokio` | 1.x | Async runtime |
| `serde` | 1.x | Serialization |

## Embeddings

Uses Gemini `text-embedding-004` model:
- 1536 dimensions
- Free tier available
- Batching support (up to 100 texts)

```rust
// src/embeddings.rs
pub struct Embeddings {
    api_key: String,
    http_client: reqwest::Client,
}

impl Embeddings {
    pub async fn embed(&self, text: &str) -> Result<Vec<f32>>;
    pub async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
}
```

## Code Indexing

Tree-sitter based parsing for:
- **Rust**: functions, structs, enums, traits, impl blocks
- **Python**: functions, classes
- **TypeScript/TSX**: functions, classes, interfaces
- **JavaScript/JSX**: functions, classes
- **Go**: functions, methods, structs, interfaces

Symbols stored in `code_symbols` table with:
- File path
- Symbol name and type
- Line range
- Optional signature

## Hooks

### Permission Hook

Auto-approves tool calls based on saved rules:

```bash
mira hook permission
```

Reads from stdin, checks `permission_rules` table, outputs allow/deny.

## Configuration

### Environment Variables

| Variable | Description |
|----------|-------------|
| `OPENAI_API_KEY` | OpenAI API key for embeddings |
| `GOOGLE_API_KEY` | Alternative to OPENAI_API_KEY |

### File Paths

| Path | Purpose |
|------|---------|
| `~/.mira/mira.db` | SQLite database |
| `.mcp.json` | MCP server configuration |

## Design Decisions

### Why rusqlite + sqlite-vec?

Previous architecture used sqlx + Qdrant:
- Required running Qdrant service
- Complex async connection management
- 67+ tables with unused features

New architecture:
- Single SQLite file
- sqlite-vec for embeddings (no external service)
- 14 tables total
- Simpler deployment

### Why rmcp?

- Official Rust MCP SDK
- Macro-based tool registration
- Handles JSON-RPC protocol
- Stdio transport built-in

### Why tree-sitter?

- Battle-tested parsers
- Incremental parsing
- Multi-language support
- Static analysis (no runtime needed)

## Background Worker

The background worker runs when the service starts and processes work during idle time:

```
┌─────────────────────────────────────────────────────────────────┐
│  Background Worker (spawns on service start)                   │
│                                                                 │
│  Every 60s (idle) / 5s (active):                               │
│    1. Check for pending embeddings                             │
│       - Create OpenAI Batch API job (50% cheaper)              │
│       - Poll for completion, store results                     │
│    2. Check for modules without summaries                      │
│       - Rate-limited DeepSeek calls                            │
│       - Update codebase_modules.purpose                        │
└─────────────────────────────────────────────────────────────────┘
```

### Real-time Fallback

When `semantic_code_search` is called before batch completes:
1. Check `pending_embeddings` for active project
2. Embed up to 50 chunks inline (immediate)
3. Delete from pending queue
4. Search runs with fresh embeddings

This ensures search always works, even if user starts before batch completes.

### Cost Savings

| Operation | Normal API | Batch API | Savings |
|-----------|-----------|-----------|---------|
| Embeddings | $0.02/1M tokens | $0.01/1M tokens | 50% |

Batch API has 24h turnaround but is processed faster in practice.
