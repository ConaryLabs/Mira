# Mira Architecture

**Version**: 3.2.0
**Last Updated**: 2026-01-05

## Overview

Mira is an MCP (Model Context Protocol) server that provides persistent memory and code intelligence for Claude Code. It also provides a web chat interface powered by DeepSeek Reasoner. Both interfaces share a unified tool core for consistent behavior.

## Source Structure

```
crates/mira-server/src/
├── main.rs           # CLI entry point (serve, index, hook, web)
├── lib.rs            # Library exports
├── db/               # Database layer (rusqlite + sqlite-vec)
│   ├── mod.rs        # Database struct and queries
│   ├── project.rs    # Project CRUD operations
│   └── tasks.rs      # Task/Goal CRUD operations
├── embeddings.rs     # OpenAI embeddings API client
├── background/       # Background worker for batch processing
│   ├── mod.rs        # Worker loop, spawns on service start
│   ├── watcher.rs    # File system watcher
│   └── ...
├── cartographer/     # Codebase structure mapping
│   └── mod.rs        # Module detection, dependency graphs
├── tools/            # UNIFIED TOOL CORE (new)
│   ├── core/         # Shared tool implementations
│   │   ├── mod.rs    # ToolContext trait
│   │   ├── memory.rs # recall, remember, forget
│   │   ├── code.rs   # search_code, find_callers/callees
│   │   ├── project.rs# set_project, get_project, list_projects
│   │   ├── tasks_goals.rs # task/goal CRUD
│   │   ├── web.rs    # google_search, web_fetch, research
│   │   ├── claude.rs # claude_task, claude_close, claude_status
│   │   └── bash.rs   # bash command execution
│   ├── web.rs        # ToolContext impl for AppState
│   └── mcp.rs        # ToolContext impl for MiraServer
├── mcp/
│   ├── mod.rs        # MCP server (rmcp)
│   └── tools/        # MCP tool handlers (delegate to core)
│       ├── mod.rs
│       ├── project.rs   # session_start, set_project, get_project
│       ├── memory.rs    # remember, recall, forget
│       ├── code.rs      # get_symbols, semantic_code_search, index
│       └── tasks.rs     # task, goal
├── web/              # Web server (axum)
│   ├── mod.rs        # HTTP routes
│   ├── state.rs      # AppState
│   ├── deepseek.rs   # DeepSeek Reasoner client
│   └── chat/
│       ├── mod.rs    # Chat API endpoints
│       └── tools.rs  # Web tool handlers (delegate to core)
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
Claude Code                    Web Browser
    │                              │
    ▼ (stdio)                      ▼ (HTTP/WS)
┌──────────────────┐     ┌──────────────────────┐
│  MCP Server      │     │  Web Server (axum)   │
│  - JSON-RPC      │     │  - REST API          │
│  - rmcp          │     │  - WebSocket events  │
└────────┬─────────┘     └──────────┬───────────┘
         │                          │
         │   ┌──────────────────┐   │
         └──►│ Unified Tool Core│◄──┘
             │  ToolContext trait│
             │  - memory.rs      │
             │  - code.rs        │
             │  - tasks_goals.rs │
             │  - project.rs     │
             └────────┬─────────┘
                      │
         ┌────────────┼────────────┐
         ▼            ▼            ▼
    ┌─────────┐ ┌──────────┐ ┌───────────┐
    │ SQLite  │ │sqlite-vec│ │  OpenAI   │
    │ Tables  │ │ Vectors  │ │ Embeddings│
    └─────────┘ └──────────┘ └───────────┘
```

## Unified Tool Core

The `tools/core/` module provides a single implementation of all tools that works with both interfaces:

```rust
// ToolContext trait abstracts the differences between MCP and Web
#[async_trait]
pub trait ToolContext: Send + Sync {
    fn db(&self) -> &Arc<Database>;
    fn embeddings(&self) -> Option<&Arc<Embeddings>>;
    async fn get_project(&self) -> Option<ProjectContext>;
    async fn set_project(&self, project: ProjectContext);
    // ... other shared resources
}

// Both AppState (web) and MiraServer (MCP) implement ToolContext
impl ToolContext for AppState { ... }
impl ToolContext for MiraServer { ... }

// Tool functions are generic over ToolContext
pub async fn recall<C: ToolContext>(ctx: &C, query: String, ...) -> Result<String, String>
```

This ensures:
- **Consistent behavior** across MCP and web chat
- **Single source of truth** for each tool
- **Easy maintenance** - update once, works everywhere

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

## Tools

### Shared Tools (MCP + Web Chat)

| Tool | Description | Core Module |
|------|-------------|-------------|
| `recall` | Semantic search through memories | `memory.rs` |
| `remember` | Store a memory fact | `memory.rs` |
| `forget` | Delete a memory by ID | `memory.rs` |
| `semantic_code_search` | Search code by meaning | `code.rs` |
| `find_callers` | Find functions that call a function | `code.rs` |
| `find_callees` | Find functions called by a function | `code.rs` |
| `set_project` | Set active project | `project.rs` |
| `get_project` | Get current project | `project.rs` |
| `list_projects` | List all projects | `project.rs` |
| `task` | Manage tasks (create/list/update/complete/delete) | `tasks_goals.rs` |
| `goal` | Manage goals (create/list/update/progress/delete) | `tasks_goals.rs` |

### MCP-Only Tools

| Tool | Description |
|------|-------------|
| `session_start` | Initialize session with project context |
| `session_history` | Query session history |
| `get_session_recap` | Get session recap for system prompts |
| `get_symbols` | Get symbols from a file (tree-sitter) |
| `index` | Index project code |
| `summarize_codebase` | Generate LLM summaries for modules |

### Web Chat-Only Tools

| Tool | Description | Core Module |
|------|-------------|-------------|
| `claude_task` | Send task to Claude Code instance | `claude.rs` |
| `claude_close` | Close Claude Code instance | `claude.rs` |
| `claude_status` | Get Claude Code status | `claude.rs` |
| `discuss` | Discuss with Claude (collaboration) | `claude.rs` |
| `google_search` | Search the web | `web.rs` |
| `web_fetch` | Fetch and parse a URL | `web.rs` |
| `research` | Multi-step research pipeline | `web.rs` |
| `bash` | Execute shell commands | `bash.rs` |

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
