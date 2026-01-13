# Mira Architecture

**Version**: 4.0.0
**Last Updated**: 2026-01-12

## Overview

Mira is an MCP (Model Context Protocol) server that provides persistent memory and code intelligence for Claude Code. It runs as a stdio-based MCP server spawned by Claude Code.

## Source Structure

```
crates/mira-server/src/
├── main.rs           # CLI entry point (serve, index, hook)
├── lib.rs            # Library exports
├── db/               # Database layer (rusqlite + sqlite-vec)
│   ├── mod.rs        # Database struct and queries
│   ├── project.rs    # Project CRUD operations
│   ├── tasks.rs      # Task/Goal CRUD operations
│   ├── memory.rs     # Memory facts CRUD
│   ├── session.rs    # Session tracking
│   └── schema.rs     # Schema definitions
├── embeddings.rs     # OpenAI embeddings API client
├── background/       # Background worker for batch processing
│   ├── mod.rs        # Worker loop, spawns on MCP start
│   ├── watcher.rs    # File system watcher
│   ├── embeddings.rs # Batch embedding processing
│   ├── summaries.rs  # Module summary generation
│   ├── briefings.rs  # Git change briefings
│   ├── capabilities.rs # Capability scanning
│   └── code_health/  # Code health analysis
├── cartographer/     # Codebase structure mapping
│   ├── mod.rs        # Module detection, dependency graphs
│   ├── detection.rs  # Language-specific detection
│   ├── map.rs        # Module map generation
│   └── summaries.rs  # LLM-powered summaries
├── tools/            # Tool implementations
│   ├── core/         # Shared tool logic
│   │   ├── mod.rs    # ToolContext trait
│   │   ├── memory.rs # recall, remember, forget
│   │   ├── code.rs   # search_code, find_callers/callees
│   │   ├── project.rs# set_project, get_project
│   │   ├── tasks_goals.rs # task/goal CRUD
│   │   └── experts.rs # Expert consultation
│   └── mcp.rs        # MCP-specific adapters
├── mcp/
│   ├── mod.rs        # MCP server (rmcp)
│   ├── extraction.rs # Tool memory extraction
│   └── tools/        # MCP tool handlers
│       ├── mod.rs
│       ├── project.rs   # session_start, set_project, get_project
│       ├── memory.rs    # remember, recall, forget
│       ├── code.rs      # get_symbols, semantic_code_search, index
│       ├── tasks.rs     # task, goal
│       └── experts.rs   # consult_* tools
├── llm/              # LLM clients
│   └── deepseek/     # DeepSeek Reasoner for experts
├── search/           # Unified search layer
│   ├── mod.rs        # Exports
│   ├── semantic.rs   # Vector search
│   ├── keyword.rs    # Text search
│   └── crossref.rs   # Call graph queries
├── indexer/
│   ├── mod.rs        # Code indexing orchestration
│   └── parsers/      # Tree-sitter parsers
└── hooks/
    └── permission.rs # Auto-approval hook
```

## Data Flow

```
Claude Code
    │
    ▼ (stdio JSON-RPC)
┌──────────────────┐
│  MCP Server      │
│  - rmcp          │
│  - Tool handlers │
└────────┬─────────┘
         │
         ▼
┌──────────────────┐
│ Tool Core        │
│  - memory.rs     │
│  - code.rs       │
│  - tasks_goals.rs│
│  - experts.rs    │
└────────┬─────────┘
         │
┌────────┼────────────┐
▼        ▼            ▼
SQLite   sqlite-vec   DeepSeek
Tables   Vectors      (experts)
```

## Database Schema

### Regular Tables

```sql
-- Projects
projects (id, path, name, created_at)

-- Memory
memory_facts (id, project_id, key, content, fact_type, category, confidence, has_embedding, created_at, updated_at)
corrections (id, project_id, what_was_wrong, what_is_right, correction_type, scope, confidence, created_at)

-- Code Intelligence
code_symbols (id, project_id, file_path, name, symbol_type, start_line, end_line, signature, indexed_at)
call_graph (id, caller_id, callee_name, callee_id, call_count)
imports (id, project_id, file_path, import_path, is_external)
codebase_modules (id, project_id, module_id, name, path, purpose, exports, dependencies, line_count)

-- Sessions
sessions (id, project_id, status, summary, started_at, last_activity)
tool_history (id, session_id, tool_name, arguments, result_summary, full_result, success, created_at)

-- Tasks & Goals
goals (id, project_id, title, description, status, priority, progress_percent, created_at)
milestones (id, goal_id, title, weight, completed, completed_at)
tasks (id, project_id, goal_id, title, description, status, priority, created_at)

-- Background Processing
pending_embeddings (id, project_id, file_path, chunk_content, status, created_at)
background_batches (id, batch_id, item_ids, status, created_at)

-- Server State
server_state (key, value, updated_at)
```

### Vector Tables (sqlite-vec)

```sql
-- 1536-dimension OpenAI embeddings
vec_memory (embedding, fact_id, content)
vec_code (embedding, file_path, chunk_content, project_id)
```

## MCP Tools

| Tool | Description |
|------|-------------|
| `session_start` | Initialize session with project context |
| `session_history` | Query session history |
| `get_session_recap` | Get session recap for system prompts |
| `remember` | Store a memory fact |
| `recall` | Semantic search through memories |
| `forget` | Delete a memory by ID |
| `get_symbols` | Get symbols from a file (tree-sitter) |
| `semantic_code_search` | Search code by meaning |
| `find_callers` | Find functions that call a function |
| `find_callees` | Find functions called by a function |
| `check_capability` | Check if a feature exists in codebase |
| `index` | Index project code |
| `summarize_codebase` | Generate LLM summaries for modules |
| `set_project` | Set active project |
| `get_project` | Get current project |
| `task` | Manage tasks (create/bulk_create/list/update/complete/delete) |
| `goal` | Manage goals (create/bulk_create/list/update/progress/delete) |
| `consult_architect` | System design consultation (DeepSeek Reasoner) |
| `consult_code_reviewer` | Code review consultation |
| `consult_security` | Security analysis consultation |
| `consult_scope_analyst` | Requirements gap analysis |
| `consult_plan_reviewer` | Plan validation |
| `reply_to_mira` | Reply during collaboration |

## Key Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| `rmcp` | 0.12 | MCP server framework |
| `rusqlite` | 0.32 | SQLite database |
| `sqlite-vec` | 0.1.6 | Vector embeddings extension |
| `tree-sitter` | 0.24 | Code parsing |
| `reqwest` | 0.12 | HTTP client (OpenAI, DeepSeek) |
| `tokio` | 1.x | Async runtime |
| `serde` | 1.x | Serialization |

## Embeddings

Uses OpenAI `text-embedding-3-small` model:
- 1536 dimensions
- Batching support

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
| `DEEPSEEK_API_KEY` | DeepSeek API key for expert consultation |

### File Paths

| Path | Purpose |
|------|---------|
| `~/.mira/mira.db` | SQLite database |
| `.mcp.json` | MCP server configuration |

## Design Decisions

### Why MCP-only (stdio)?

Previous architecture had HTTP-based MCP:
- Required running a separate web server
- Complex deployment with systemd service
- Additional attack surface

New architecture:
- Claude Code spawns Mira directly via stdio
- No separate service to manage
- Simpler deployment (just the binary)
- Background worker spawns within MCP server

### Why rusqlite + sqlite-vec?

Previous architecture used sqlx + Qdrant:
- Required running Qdrant service
- Complex async connection management
- 67+ tables with unused features

New architecture:
- Single SQLite file
- sqlite-vec for embeddings (no external service)
- Simplified schema
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

The background worker spawns when the MCP server starts:

```
┌─────────────────────────────────────────────────────────────────┐
│  Background Worker (spawns on MCP start)                        │
│                                                                 │
│  Periodic tasks:                                                │
│    1. Check for pending embeddings                              │
│       - Batch embed with OpenAI                                 │
│       - Store results in vec_code                               │
│    2. Check for modules without summaries                       │
│       - Generate with DeepSeek                                  │
│       - Update codebase_modules.purpose                         │
│    3. Scan for capabilities                                     │
│       - Identify features in codebase                           │
│       - Flag incomplete implementations                         │
└─────────────────────────────────────────────────────────────────┘
```
