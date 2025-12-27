# **Mira: Complete System Architecture**

| | |
|---|---|
| **Version** | 2.0.0 |
| **Last Updated** | 2025-12-27 |
| **Rust Edition** | 2024 |
| **Document Revision** | 1.4 |

> **Maintenance Note**: Update this document after major architectural changes. Bump revision for minor updates, version for breaking changes.

## 1. **Executive Summary**

**Core Principle**: All business logic lives in `src/core/ops/`. Both MCP (Claude Code integration) and Studio/Chat are thin wrappers over shared operations.

> **What "thin wrapper" means**: MCP and Chat layers handle only protocol translation (JSON-RPC ↔ HTTP/SSE), authentication, and request routing. They do NOT contain business logic, state management, or persistence calls. All of that lives in `core/ops/`. If you're adding a feature, it goes in `core/ops/`—the wrappers just expose it.

**Project timeline**: Multiple iterations before July 2025, current codebase started 2025-07-18.

**Key architectural phases**:
1. **2025-07**: Foundation - GPT-4.1, Qdrant, persona system, semantic memory
2. **2025-08**: GPT-5 migration, modularization, tool integration
3. **2025-09-10**: Claude -> DeepSeek experiments, code intelligence, layered context
4. **2025-11**: Architecture overhaul, GPT 5.1, intelligence milestones 2-9
5. **2025-12**: MCP/Studio convergence, core/ops unification, Carousel v2

**System boundaries**:
- **Inputs**: MCP protocol (rmcp), HTTP chat API, REPL commands
- **Outputs**: LLM responses, tool execution results, persistence (SQLite, Qdrant)
- **External**: LLM providers (OpenAI, DeepSeek, Google), Qdrant vector DB

## 2. **Module Hierarchy & Responsibilities**

### Complete Source Tree (`src/`)
```
src/
├── main.rs                 # Binary entry - CLI/env/TOML merge, SQLite+Qdrant init, REPL/HTTP
├── lib.rs                  # Library root (new)
├── daemon.rs               # Daemon logic (286 lines, lean)
├── connect.rs              # Connection utilities
│
├── batch/                  # Batch processing (50% cost savings via Gemini Batch API)
│   ├── mod.rs
│   └── worker.rs           # Background worker for async batch jobs
│
├── chat/                   # Chat interface (Mira-Chat)
│   ├── mod.rs
│   ├── context.rs
│   ├── conductor/
│   │   ├── mod.rs
│   │   └── validation.rs
│   ├── provider/          # LLM providers
│   │   ├── mod.rs
│   │   ├── types.rs
│   │   ├── capabilities.rs
│   │   ├── batch.rs      # Gemini Batch API client (50% cost savings)
│   │   ├── file_search.rs # Gemini FileSearch API client (RAG)
│   │   ├── deepseek.rs   # DeepSeek V3.2 integration
│   │   └── responses.rs
│   ├── session/          # Conversation management
│   │   ├── mod.rs
│   │   ├── types.rs
│   │   ├── chain.rs
│   │   ├── context.rs
│   │   ├── messages.rs
│   │   ├── compaction.rs
│   │   ├── summarization.rs
│   │   ├── freshness.rs
│   │   ├── git_tracker.rs
│   │   ├── code_hints.rs
│   │   ├── graph.rs
│   │   ├── anti_amnesia.rs
│   │   └── errors.rs
│   ├── server/           # HTTP server
│   │   ├── mod.rs
│   │   ├── chat.rs
│   │   ├── handlers.rs
│   │   ├── stream.rs
│   │   ├── types.rs
│   │   └── markdown_parser.rs
│   └── tools/            # Chat-accessible tools
│       ├── mod.rs        # Tool registration
│       ├── types.rs
│       ├── mira.rs
│       ├── memory.rs
│       ├── file.rs
│       ├── shell.rs
│       ├── git.rs
│       ├── code_intel.rs
│       ├── build.rs
│       ├── documents.rs
│       ├── git_intel.rs
│       ├── index.rs
│       ├── test.rs
│       ├── proactive.rs
│       ├── definitions.rs
│       └── tool_defs/
│           ├── mod.rs
│           ├── mira.rs
│           ├── memory.rs
│           ├── file_ops.rs
│           ├── intel.rs
│           └── testing.rs
│
├── core/                  # Shared business logic
│   ├── mod.rs
│   ├── context.rs
│   ├── error.rs
│   ├── primitives/
│   │   ├── mod.rs
│   │   ├── memory.rs      # Memory fact structures
│   │   ├── semantic.rs    # Qdrant + embedding search
│   │   ├── semantic_helpers.rs
│   │   ├── artifacts.rs   # Large output storage
│   │   ├── excerpts.rs
│   │   ├── limits.rs
│   │   ├── secrets.rs
│   │   └── streaming.rs   # SSE decoder
│   └── ops/              # **CENTRAL BUSINESS LOGIC**
│       ├── mod.rs        # Operation definitions
│       ├── memory.rs
│       ├── file.rs
│       ├── shell.rs
│       ├── git.rs
│       ├── code_intel.rs
│       ├── build.rs
│       ├── tasks.rs
│       ├── goals.rs
│       ├── corrections.rs
│       ├── decisions.rs
│       ├── rejected.rs
│       ├── session.rs
│       ├── chat_chain.rs
│       ├── chat_summary.rs
│       ├── documents.rs
│       ├── proposals.rs
│       ├── work_state.rs
│       └── audit.rs
│
├── context/              # Context management
│   ├── mod.rs
│   └── carousel.rs      # Carousel v2 - semantic interrupts, panic mode (1191 lines)
│
├── hooks/               # Runtime hooks
│   ├── mod.rs
│   ├── pretool.rs       # PreToolUse for auto code context
│   ├── posttool.rs      # PostToolUse for passive context building
│   ├── precompact.rs
│   ├── sessionstart.rs
│   └── permission.rs
│
├── indexer/             # Code/git indexing
│   ├── mod.rs
│   ├── code.rs
│   ├── git.rs
│   ├── watcher.rs
│   └── parsers/
│       ├── mod.rs
│       ├── rust.rs
│       ├── python.rs
│       ├── typescript.rs
│       └── go.rs
│
├── server/              # HTTP server components
│   ├── mod.rs
│   ├── db.rs
│   └── handlers/
│       ├── mod.rs
│       ├── indexing.rs
│       └── proposals.rs
│
├── spawner/             # Claude Code session spawner
│   ├── mod.rs           # Module exports
│   ├── process.rs       # Process lifecycle (spawn, inject, terminate)
│   ├── stream.rs        # Stream-JSON parser for Claude output
│   └── types.rs         # SpawnConfig, SessionStatus, SessionEvent
│
├── tools/               # MCP tool implementations
│   ├── mod.rs
│   ├── types.rs
│   ├── batch.rs        # Batch processing tool handler
│   ├── memory.rs
│   ├── file.rs
│   ├── shell.rs
│   ├── git.rs
│   ├── code_intel.rs
│   ├── build_intel.rs
│   ├── tasks.rs
│   ├── goals.rs
│   ├── corrections.rs
│   ├── documents.rs
│   ├── sessions.rs
│   ├── work_state.rs
│   ├── proactive.rs
│   ├── project.rs
│   ├── ingest.rs
│   ├── mcp_history.rs
│   ├── response.rs
│   ├── permissions.rs
│   ├── helpers.rs
│   ├── analytics.rs
│   └── format/          # Formatting utilities
│       ├── mod.rs
│       ├── memory.rs
│       ├── code.rs
│       ├── sessions.rs
│       ├── entities.rs
│       ├── tests.rs
│       ├── proactive.rs
│       └── admin.rs
│
└── [root .rs files already listed]
```

### External Directories
```
examples/compare_tools.rs   # Tool comparison example
tests/                      # Integration tests (daemon_e2e, integration_e2e, etc.)
data/                       # SQLite, Qdrant persistence
docs/ARCHITECTURE.md        # Architecture documentation
```

### Module Interdependencies
```
MCP Server (src/tools/) → core::ops → core::primitives
Chat Interface (src/chat/) → core::ops → core::primitives
Carousel Context → chat/session/context → core::ops
```

### Ownership Boundaries
1. **core::ops**: Owns all business logic, data structures, persistence
2. **core::primitives**: Utilities with no external dependencies
3. **src/tools/**: MCP protocol adapters (type conversion only)
4. **src/chat/tools/**: Chat protocol adapters (type conversion only)

### Cross-cutting Concerns
- **Error handling**: `core::error.rs` defines unified error types
- **Configuration**: CLI/env/TOML merge in `main.rs`
- **Telemetry**: SQLite `chat_usage` table, `/usage` REPL command

## 3. **Data Flow & State Management**

Mira implements a **dual-entry architecture**: both MCP (Claude Code) and Chat (Studio/HTTP) share identical business logic in `src/core/ops/`, with only thin protocol adapters at the edges.

### **3.1 Two Entry Points → One Core → Two Exit Paths**

#### **MCP Entry Path (Claude Code Integration)**
```
Claude Code → rmcp protocol → src/tools/mod.rs (MCP tool registration)
  ↓ calls wrapper (e.g., src/tools/memory.rs)
  ↓ converts MCP types → common Rust types
  ↓ calls core/ops/memory.rs::upsert_memory()
  ↓ executes via OpContext (SQLite + Qdrant + HTTP client)
  ↓ returns result → wrapper converts back → rmcp → Claude Code
```

**Key files**: `tools/mod.rs` registers 17 MCP tools, each wrapper (~50 lines) handles type conversion only.

#### **Chat Entry Path (Studio/HTTP)**
```
Studio frontend → POST /api/chat/stream → src/chat/server/handlers.rs
  ↓ parses request, creates Session
  ↓ routes to LLM provider (GPT-5.2 or DeepSeek V3.2)
  ↓ when tool call received: src/chat/tools/mod.rs (tool registration)
  ↓ converts chat tool schema → common types
  ↓ calls same core/ops/*.rs functions as MCP path
  ↓ returns → HTTP/SSE stream → Studio
```

**Key files**: `chat/tools/mod.rs` registers same 17 tools with chat-specific schemas.

### **3.2 The Convergence Point: `core/ops/`**
Both entry paths hit **exact same functions** in `src/core/ops/`:
- `memory.rs::upsert_memory()` / `recall_memory()`
- `file.rs::read_file()` / `write_file()`
- `git.rs::git_status()` / `git_commit()`
- `code_intel.rs::find_similar_fixes()`
- `build_intel.rs::record_build_error()`
- etc. (18 total operation modules)

**OpContext struct** bundles shared resources:
```rust
pub struct OpContext {
    pub db: SqliteConnection,      // ~/.mira/mira.db
    pub semantic: QdrantClient,    // Vector search
    pub http: reqwest::Client,     // External APIs
    pub secrets: SecretsManager,   // Encrypted credentials
}
```

### **3.3 Persistence Layer Architecture**

#### **SQLite Database (`~/.mira/mira.db`)**
```
chat_usage          # Token telemetry (input/output/cached/chain/flags)
artifacts           # Large tool outputs (>4KB) with smart previews
memories            # Semantic memories (text + embedding_id)
corrections         # Style/approach corrections
goals               # High-level goals with milestones
decisions           # Architecture decisions with rationale
sessions            # Chat session state
context_slices      # Carousel v2 context windows
batch_jobs          # Async batch processing jobs (compaction, summarize, analyze)
batch_requests      # Individual requests within a batch job
file_search_stores  # Gemini FileSearch stores per project
file_search_documents # Indexed documents in FileSearch stores
instruction_queue   # Pending instructions for Claude Code (with session_id link)
claude_sessions     # Spawned Claude Code session state
```

#### **Qdrant Vector Database**
- Memory embeddings using Gemini free-tier embeddings
- Semantic recall via `recall_memory()` → `semantic_code_search()`
- Context assembly pulls top-k relevant memories

### **3.4 Context Management Flow (Carousel v2)**

The Carousel is a **deterministic state machine** that manages what context gets injected into LLM prompts. It replaces the earlier time-based decay heuristics that caused unpredictable "amnesia."

#### **State Machine**
```
                    ┌─────────────┐
                    │   NORMAL    │ ← Default state
                    └──────┬──────┘
                           │
         ┌─────────────────┼─────────────────┐
         ▼                 ▼                 ▼
   ┌──────────┐     ┌──────────┐      ┌──────────┐
   │  PINNED  │     │  PANIC   │      │ FOCUSED  │
   │ (manual) │     │ (token   │      │ (semantic│
   └──────────┘     │  spike)  │      │ interrupt)
                    └──────────┘      └──────────┘
```

#### **Selection Algorithm**
```
User request → src/context/carousel.rs::select_context_slice()
  ↓ checks in order:
  1) Semantic interrupts (query→category match, e.g., "error" → "debugging")
  2) Panic mode (token spike > threshold → force compact context)
  3) Trigger overrides (manual pins via /pin command)
  4) Starvation prevention (LRU cold start for unused contexts)
  ↓ selects optimal context slice
  ↓ assembles full prompt with:
     - Corrections (always first, stable)
     - Goals (current active)
     - Semantic memories from Qdrant (top 5 by relevance)
     - Artifact previews (head/tail + grep matches, not full content)
     - Recent conversation summary
  ↓ sends to LLM with token budget enforcement
```

#### **Context Composition Strategy**

Context assembly follows a strict priority order to ensure critical information is never crowded out:

| Priority | Category | Max Tokens | Rationale |
|----------|----------|------------|-----------|
| 1 | Corrections | 500 | Style/approach rules must always apply |
| 2 | Active goals | 300 | Current objectives guide all work |
| 3 | Pinned anchors | 200 | User-specified critical context |
| 4 | Recent messages | 2000 | Conversational continuity |
| 5 | Semantic memories | 1500 | Relevant past knowledge |
| 6 | Summaries | 1000 | Compressed older context |
| 7 | Artifact previews | 500 | Tool output snippets |

**Token budget enforcement**: If total exceeds budget, lower-priority categories are truncated first. Corrections and goals are never truncated—if they alone exceed budget, that's a configuration error.

### **3.5 Artifact System (Cost Control)**
```
Tool output >4KB → src/core/ops/artifact.rs::store_artifact()
  ↓ stores full content in SQLite artifacts table
  ↓ generates smart preview:
     - grep top 3 matches if text searchable
     - diff hunks if patch output
     - bash metadata (exit code, duration) if command
     - head (first 200 chars) + tail (last 200 chars)
  ↓ includes preview in prompt (truncated to 1KB)
  ↓ full content accessible via:
     - fetch_artifact(artifact_id, offset, limit)
     - search_artifact(artifact_id, query)
```

**Why**: Prevents 20KB git diff from blowing up token costs.

### **3.6 Real Example: "Find similar errors in Rust code"**
```
# MCP PATH
Claude Code → tools/code_intel.rs::handle_find_similar_fixes()
  ↓ converts MCP params → FindSimilarFixesArgs
  ↓ calls core/ops/code_intel.rs::find_similar_fixes()
  ↓ searches: SQLite error_fixes + Qdrant semantic matches
  ↓ returns Vec<ErrorFix> → MCP wrapper → rmcp → Claude

# CHAT PATH  
Studio → chat/tools/code_intel.rs::handle_find_similar_fixes()
  ↓ converts chat tool call → same FindSimilarFixesArgs
  ↓ calls SAME core/ops/code_intel.rs::find_similar_fixes()
  ↓ SAME search logic
  ↓ returns Vec<ErrorFix> → chat wrapper → HTTP/SSE → Studio
```

**Identical code path after type conversion.**

### **3.7 Sync Between Instances (Claude ↔ Mira)**
```
Claude → POST /api/chat/sync (src/chat/server/handlers.rs)
  - Bearer auth required
  - Body: { messages, response_id?, previous_response_id? }
  - Rate limiting: 1 concurrent request per session
  - Structured JSON errors (never plain text)
  - Returns: { response_id, previous_response_id, chain_id, request_id, timestamp }

Chain tracking:
  response_id → SHA256 of response
  previous_response_id → links to prior response
  chain_id → derived from thread (detects when >10 deep → soft reset)
  Soft reset: token threshold + "handoff blob" (summary) → no obvious amnesia
```

**Prevents "split-brain"**: Multiple Mira instances share same SQLite DB via symlink `~/.mira/mira.db → /home/peter/Mira/data/mira.db`.

### **3.8 State Management & Concurrency**
- **Session state**: In-memory HashMap keyed by session_id, 24h TTL
- **SQLite connections**: Connection pool (r2d2), max 10 connections
- **Qdrant**: Single client, connection pooling internal
- **Tool execution**: Sequential per session (no parallel tool calls)
- **Memory safety**: All `unwrap()` removed from production code, `.expect()` with descriptive messages only

### **3.9 Telemetry Flow**
```
Every LLM response → src/chat/usage.rs::record_usage()
  ↓ inserts into chat_usage table:
     - session_id, response_id, previous_response_id
     - input_tokens, output_tokens, cached_tokens
     - cache_percent, total_cost (microdollars)
     - flags: spike, cache_drop, new_chain, tools_used
  ↓ REPL command: `/usage` queries this table
  ↓ Spike detection: flags.spike = true if input_tokens > 2× moving average
```

**Observability**: `/usage` shows chain visualization, cost breakdown, anomaly detection.

## 4. **Configuration Inventory**

Mira uses environment variables for configuration, with sensible defaults. No TOML config files.

### **4.1 Environment Variables**

#### **Database & Storage**
| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `DATABASE_URL` | No | `sqlite://~/.mira/mira.db` | SQLite connection string |
| `QDRANT_URL` | No | None (disables semantic search) | Qdrant gRPC endpoint (e.g., `http://localhost:6334`) |
| `MIRA_MIGRATIONS_DIR` | No | `./migrations` or relative to binary | Custom migrations path |

#### **API Keys (LLM Providers)**
| Variable | Required | Description |
|----------|----------|-------------|
| `GEMINI_API_KEY` | Yes | Google Gemini API key (for embeddings, chat, file search) |
| `GOOGLE_API_KEY` | Fallback | Alternative to GEMINI_API_KEY |

#### **Server Configuration**
| Variable | Default | Description |
|----------|---------|-------------|
| `MIRA_PORT` | `3000` | HTTP server port |
| `MIRA_LISTEN` | `127.0.0.1` | Bind address (`0.0.0.0` to expose externally) |
| `MIRA_URL` | `http://localhost:3000` | Daemon URL for CLI commands |
| `MIRA_SYNC_TOKEN` | Auto-generated | Auth token for sync endpoint (saved to `~/.mira/token`) |
| `MIRA_CORS_ORIGINS` | `*` (localhost) | Comma-separated allowed origins |
| `RUST_LOG` | `info` | Log level (`debug`, `info`, `warn`, `error`) |

### **4.2 CLI Commands**

```bash
# Start daemon (default command)
mira daemon [--port 3000] [--listen 127.0.0.1]

# Connect stdio to running daemon (for Claude Code MCP)
mira connect [--url http://localhost:3000]

# Check daemon status
mira status [--url http://localhost:3000]

# Stop running daemon
mira stop [--url http://localhost:3000]

# Hook handlers (called by Claude Code settings.json)
mira hook permission    # Auto-approve based on saved rules
mira hook precompact    # Save context before compaction
mira hook posttool      # Auto-remember significant actions
mira hook pretool       # Provide code context before file ops
mira hook sessionstart  # Check for unfinished work
```

### **4.3 File Paths**

| Path | Purpose |
|------|---------|
| `~/.mira/mira.db` | SQLite database (symlinked to project data/) |
| `~/.mira/token` | Auto-generated auth token (32 bytes, hex-encoded) |
| `~/.mira/.env` | Environment file (loaded by systemd service) |
| `/home/peter/Mira/data/` | Actual data directory (symlink target) |
| `/home/peter/Mira/migrations/` | SQLx migration files |

### **4.4 Hardcoded Limits**

#### **Server Limits**
| Constant | Value | Location |
|----------|-------|----------|
| `DEFAULT_PORT` | 3000 | `main.rs:46` |
| `SYNC_MAX_BODY_BYTES` | 64KB | `chat/server/mod.rs:180` |
| `SYNC_MAX_MESSAGE_BYTES` | 32KB | `chat/server/stream.rs:82` |
| `SYNC_MAX_CONCURRENT` | 3 | `chat/server/mod.rs:183` |

#### **Tool Limits**
| Constant | Value | Location |
|----------|-------|----------|
| `MAX_READ_SIZE` | 1MB | `core/ops/file.rs:15` |
| `MAX_OUTPUT_SIZE` | 64KB | `core/ops/shell.rs:11` |
| `MAX_FILE_SIZE` (cache) | 512KB | `chat/tools/mod.rs:210` |
| `MAX_ENTRIES` (cache) | 100 | `chat/tools/mod.rs:209` |

#### **Session/Context Limits**
| Constant | Value | Location |
|----------|-------|----------|
| `RECALL_THRESHOLD` | 0.75 | `chat/session/messages.rs:15` |
| `RECALL_LIMIT` | 3 | `chat/session/messages.rs:18` |
| `RECENT_RAW_COUNT` | 5 | `chat/session/mod.rs:48` |
| `SUMMARIZE_THRESHOLD` | 10 messages | `chat/session/mod.rs:54` |
| `MAX_SUMMARIES_IN_CONTEXT` | 5 | `chat/session/mod.rs:60` |

#### **Carousel Limits**
| Constant | Value | Location |
|----------|-------|----------|
| `MAX_STARVATION_TURNS` | 12 | `context/carousel.rs:25` |
| `ANCHOR_MAX_TOKENS` | 200 | `context/carousel.rs:28` |
| `ANCHOR_MAX_ITEMS` | 2 | `context/carousel.rs:31` |

#### **Ingestion/Chunking**
| Constant | Value | Location |
|----------|-------|----------|
| `TARGET_CHUNK_TOKENS` | 500 | `tools/ingest.rs:40` |
| `CHUNK_OVERLAP_TOKENS` | 50 | `tools/ingest.rs:41` |
| `SEMANTIC_DUPLICATE_THRESHOLD` | 0.85 | `core/ops/proposals.rs:511` |

### **4.5 Systemd Service**

The daemon runs as a user service with security hardening:

```ini
# ~/.config/systemd/user/mira.service
[Unit]
Description=Mira - Memory & Intelligence Layer (MCP + Studio)
After=network.target

[Service]
Type=simple
ExecStart=/home/peter/Mira/target/release/mira
Restart=always
RestartSec=5
Environment=MIRA_PORT=3000
EnvironmentFile=%h/.mira/.env

# Security hardening
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=read-only
ReadWritePaths=%h/.mira %h/Mira/data
PrivateTmp=true

[Install]
WantedBy=default.target
```

**Management commands:**
```bash
systemctl --user start mira
systemctl --user stop mira
systemctl --user status mira
journalctl --user -u mira -f  # Follow logs
```

### **4.6 Claude Code Integration**

MCP configuration in `.mcp.json`:
```json
{
  "mcpServers": {
    "mira": {
      "command": "/home/peter/Mira/target/release/mira",
      "args": ["connect"]
    }
  }
}
```

Hook configuration in Claude Code `settings.json`:
```json
{
  "hooks": {
    "PreToolUse": [{"matcher": {"tool_name": "Read"}, "command": "mira hook pretool"}],
    "PostToolUse": [{"matcher": {}, "command": "mira hook posttool"}],
    "SessionStart": [{"command": "mira hook sessionstart"}]
  }
}
```

## 5. **Decision Ledger**

Chronological record of architectural choices, trade-offs, and rejected alternatives.

**Pre-history**: Mira had ~3 earlier iterations that were nuked and restarted before the current codebase. The git history begins July 2025, but the conceptual work predates this.

---

### **Phase 1: Foundation (July 2025)**

#### **2025-07-18: Project Inception - GPT-4.1 + Qdrant + Persona System**
**Decision**: Build Mira as a Rust/Axum backend with GPT-4.1, Qdrant vector DB, and a persona overlay system.
- **Why**: Needed a personal AI assistant with persistent semantic memory and distinct personality.
- **Stack**: Rust 2024, Axum, SQLite for sessions, Qdrant for embeddings, GPT-4.1 for chat.
- **Key innovation**: Persona overlays with mood/intensity tracking, sub-100ms response times.

#### **2025-07-22: Sprint 1 & 2 Complete - Memory Pipeline**
**Decision**: Implement full semantic memory with persona switching, memory decay, and emotional asides.
- **Why**: Wanted Mira to remember conversations and have emotional range.
- **Features**: 3072-dimension embeddings, eternal session mode, structured JSON API.

#### **2025-07-25: VPS Migration + Memory Importer**
**Decision**: Deploy to Oregon VPS, build batch importer for ChatGPT history.
- **Why**: Needed production deployment and wanted to bootstrap Mira with existing conversation history.
- **Result**: `mira-import` tool for batch import, retcon, and evaluation.

#### **2025-07-29: Service Layer Extraction**
**Decision**: Extract service layer to eliminate REST/WebSocket duplication.
- **Why**: Code was duplicated between HTTP and WebSocket handlers.
- **Pattern**: Shared service layer called by both transport layers.

---

### **Phase 2: API Migrations (August 2025)**

#### **2025-07-31: Migrate to OpenAI Responses API**
**Decision**: Move from deprecated Assistants API to Responses API.
- **Why**: OpenAI deprecated Assistants; Responses API offered better tool integration.
- **Trade-off**: Significant refactoring required.

#### **2025-08-04: Web Search Integration**
**Decision**: Add web search via OpenAI function calling (Tavily).
- **Why**: Mira needed access to current information beyond training data.
- **Note**: Later replaced by Gemini built-in tools (2025-12-26).

#### **2025-08-09: Brief Claude Experiment (Reverted)**
**Decision**: Attempted migration to Claude + Midjourney.
- **Why**: Exploring alternatives to OpenAI.
- **Outcome**: Reverted after 2 days - API differences too significant.
- **Lesson**: Stick with OpenAI ecosystem for now.

#### **2025-08-11-13: GPT-5 Migration**
**Decision**: Migrate entire backend from GPT-4.1 to GPT-5 Responses API.
- **Why**: GPT-5 offered superior reasoning and tool use.
- **Pain**: 47 build errors to fix post-migration.
- **Result**: All modules updated, unified on `/v1/responses` endpoint.

#### **2025-08-15: Conversation Summarization**
**Decision**: Implement automatic conversation summarization.
- **Why**: Long conversations exceeded context limits; needed compression.

#### **2025-08-17: Hybrid JSON/Plain Text Approach**
**Decision**: Use structured JSON for metadata, plain text for message bodies.
- **Why**: OpenAI's structured output had 1280 token cap; messages often exceeded this.
- **Trade-off**: More complex parsing, but no token limit issues.

#### **2025-08-22: Major Modularization**
**Decision**: Refactor monolithic handlers into focused modules.
- **Results**:
  - WebSocket: 750 lines -> 4 modules (73% reduction)
  - LLM client: 650 lines -> 5 modules (77% reduction)
  - Git client: Split into modules with centralized error handling
- **Why**: Maintainability was becoming impossible.

#### **2025-08-23: Tool Integration - Image Gen + File Search**
**Decision**: Add DALL-E image generation and file search tools.
- **Why**: Expand Mira's capabilities beyond chat.

#### **2025-08-29: Multi-Head Memory Architecture**
**Decision**: Implement multi-collection support with multi-head embeddings.
- **Why**: Different types of memories (facts, preferences, code) need different treatment.
- **Phases**: Collection support -> Multi-head embeddings -> Rich metadata tagging.

---

### **Phase 3: Intelligence Systems (September-October 2025)**

#### **2025-10-01: Claude Sonnet 4.5 Migration**
**Decision**: Migrate from GPT-5 to Claude Sonnet 4.5.
- **Why**: Better code generation, prompt caching support.
- **Features**: Prompt caching implemented for cost reduction.

#### **2025-10-02: Code Intelligence System**
**Decision**: Build AST-based code intelligence with TypeScript/JavaScript parsing.
- **Why**: Mira needed to understand code structure, not just text.
- **Implementation**: SWC-based parsing, cross-language dependency tracking.

#### **2025-10-07: Layered Context Architecture**
**Decision**: Implement 5-phase memory system with layered context.
- **Phases**:
  1. Layered context with summaries
  2. Enhanced summary quality + personal context
  3. Efficiency tools + caching
  4. Simplified scoring
  5. Higher limits
- **Why**: Memory retrieval was too simplistic; needed sophistication.

#### **2025-10-08: Claude -> DeepSeek/GPT-5 Dual Architecture**
**Decision**: Replace Claude with DeepSeek + GPT-5 dual-model system.
- **Why**: Cost optimization - use DeepSeek for code, GPT-5 for voice.
- **Architecture**: GPT-5 as primary voice, DeepSeek as internal reasoning tool.
- **Phases**: Delete Claude -> Provider infrastructure -> Router -> Task classification.

#### **2025-10-09-10: GPT-5 Consolidation + ChatOrchestrator**
**Decision**: Remove DeepSeek, consolidate to GPT-5 only with ChatOrchestrator.
- **Why**: Dual-model complexity wasn't worth the cost savings.
- **Features**: Dynamic reasoning levels, SSE streaming, code intelligence layers.

---

### **Phase 4: Architecture Overhaul (November 2025)**

#### **2025-11-16: DeepSeek Integration (47 commits in one day!)**
**Decision**: Re-integrate DeepSeek with dual-model orchestration.
- **Why**: Revisited cost analysis; DeepSeek's 64K context window valuable.
- **Features**: Smart routing, activity panel, JWT auth, file operations.
- **Outcome**: Then migrated to DeepSeek-only architecture (removed GPT-5).

#### **2025-11-24: Fresh Schema + GPT 5.1 Architecture**
**Decision**: Complete architecture rewrite with fresh database schema.
- **Why**: Technical debt had accumulated; needed clean slate.
- **Features**: GPT 5.1 with reasoning effort support, new session system.

#### **2025-11-25-26: Intelligence Milestones 2-7**
**Decision**: Implement code intelligence, git intelligence, tool synthesis, build system, reasoning patterns.
- **Milestones**:
  - M2: Code Intelligence
  - M3: Git Intelligence
  - M4: Tool Synthesis
  - M5: Build System Integration
  - M6: Reasoning Pattern Learning
  - M7: Context Oracle with budget-aware config

#### **2025-11-27-28: File System + Guidelines**
**Decision**: Add real-time file watching, guidelines management, task tracking.
- **Milestones**:
  - M8: Real-time file watching
  - M9: Build errors, tools dashboard, enhanced file browser

---

### **Phase 5: MCP + Studio Convergence (December 2025)**

#### **2025-12-03: Hooks System Foundation**
**Decision**: Add extensible hooks system for pre/post tool execution.
- **Why**: Need automatic context injection and passive learning without modifying core tool logic.
- **Trade-off**: Adds indirection layer, slightly increases tool call latency.

#### **2025-12-11: Switch from OpenAI to Gemini Embeddings**
**Decision**: Replace OpenAI text-embedding-3-small with Gemini's free-tier embeddings.
- **Why**: Cost elimination (~$0.0001/1K tokens -> $0).
- **Trade-off**: Slightly slower batch encoding.

#### **2025-12-12: PreToolUse/PostToolUse Hooks in Rust**
**Decision**: Rewrite Python hooks in Rust for Claude Code integration.
- **Why**: Python hooks were slow; Rust provides type safety and speed.
- **Features**: Auto code-context injection, passive memory-building.

#### **2025-12-16: Launch mira-chat (Studio Backend)**
**Decision**: Build dedicated chat backend separate from MCP tooling.
- **Why**: Claude Code MCP is read-heavy; Studio needs streaming, multi-turn, direct model access.
- **Trade-off**: Two entry points (later unified via core/ops).

#### **2025-12-17: Artifact System + Chain Management**
**Decision**: Store large tool outputs (>4KB) in SQLite with smart previews.
- **Why**: Runaway token costs from huge outputs.
- **Also**: Smooth handoff resets, `/api/chat/sync` endpoint.

#### **2025-12-18: Hotline + Full Context Assembly**
**Decision**: Multi-provider support (GPT-5.2, DeepSeek V3.2, Gemini 3 Pro).
- **Why**: Different tasks need different models.
- **Also**: Every model gets full context (corrections, goals, memories, summaries).

#### **2025-12-19: Unified Core/Ops + Daemon Architecture**
**Decision**: Extract all business logic into shared `core/ops/` module.
- **Why**: MCP and Chat had duplicate implementations causing drift.
- **Also**: Single systemd service for all components.

#### **2025-12-20: SQLite Symlink Consolidation**
**Decision**: Symlink `~/.mira/mira.db` -> `/home/peter/Mira/data/mira.db`.
- **Why**: Multiple DB paths caused "split-brain" state.

#### **2025-12-23: DeepSeek Reasoner V3.2**
**Decision**: Route all chat traffic through DeepSeek Reasoner.
- **Why**: V3.2 added tool-call support, eliminating dual-model routing need.

#### **2025-12-25: Carousel v2**
**Decision**: Replace context-decay heuristics with deterministic state machine.
- **Features**: Semantic interrupts, panic mode, explicit overrides, starvation prevention.
- **Why**: Implicit context "bleed" confused users and models.

#### **2025-12-26: Gemini Advanced Features**
**Decision**: Replace custom web tools with Gemini's built-in capabilities.
- **Removed**: Custom `web_search`, `web_fetch` tools and infrastructure.
- **Added**: Built-in `google_search` (FREE until Jan 2026), `code_execution` (Python sandbox), `url_context` (native fetching).
- **Added**: Context caching support (~75% cost reduction on cached tokens).
- **Why**: Built-in tools are better integrated, free, and require no extra API keys.

#### **2025-12-26: Gemini FileSearch (RAG) and Batch API**
**Decision**: Add Gemini FileSearch for per-project RAG and Batch API for async processing.
- **FileSearch**: Per-project stores for semantic document search via `file_search` MCP tool.
- **Batch API**: 50% cost savings for async bulk operations (compaction, summarization, analysis).
- **Background Worker**: Polls pending jobs, submits to Gemini Batch API, processes results.
- **Thinking Levels**: Added `minimal`/`medium` thinking for Flash (beyond just `low`/`high`).
- **Cached Tokens**: Parse `cachedContentTokenCount` for accurate cost tracking.
- **Why**: Maximize Gemini 3 capabilities, reduce costs, enable advanced document search.

#### **2025-12-26: Remove Advisory/Hotline/Council System**
**Decision**: Remove multi-LLM advisory system (~10k lines).
- **Removed**: `src/advisory/`, `hotline` tool, `council` chat tools, LLM-based proposal extraction.
- **Why**: Simplification - the plan is to go through Gemini/Mira via Studio frontend, not Claude Code calling other LLMs directly.

#### **2025-12-27: Claude Code Spawner Module**
**Decision**: Add `src/spawner/` module for orchestrating Claude Code sessions from Mira.
- **Components**:
  - `process.rs`: Spawn Claude Code with `--output-format stream-json`, manage lifecycle
  - `stream.rs`: Parse stream-json events, detect `AskUserQuestion` tool calls
  - `types.rs`: `SpawnConfig`, `SessionStatus`, `SessionEvent`, context handoff types
- **HTTP Endpoints**: `/api/sessions` (spawn, list, terminate, answer, events SSE)
- **Studio Integration**: `sessions.svelte.ts` store, OrchestrationTab UI for session management
- **Database**: Added `session_id` column to `instruction_queue` for linking instructions to sessions
- **Why**: Enable Mira (via Gemini 2M context) to orchestrate Claude Code sessions for task execution, with question relay and session review.

---

**Summary Statistics**:
- **Duration**: 5+ months (July 18 - December 26, 2025)
- **Total commits**: ~800+
- **Major migrations**: GPT-4.1 -> GPT-5 -> Claude -> DeepSeek -> GPT 5.1 -> DeepSeek Reasoner
- **Architecture rewrites**: 3 (August refactor, November fresh schema, December core/ops)
- **Peak activity**: October 10 (24 commits), November 16 (47 commits)

## 6. **External Integrations**

### **6.1 LLM Provider (Gemini)**

Mira uses Google Gemini as the primary LLM provider for all chat and intelligence features.

#### **Google Gemini 3 Flash/Pro**
| Property | Flash | Pro |
|----------|-------|-----|
| API | GenerateContent API | GenerateContent API |
| Model ID | `gemini-3-flash-preview` | `gemini-3-pro-preview` |
| Cost (input/output) | $0.50/$3 per 1M | $2/$12 per 1M |
| Context | 1M tokens | 1M tokens |
| Features | Streaming, tool calling, thinking | Streaming, tool calling, thinking |
| Built-in | Google Search grounding (free until Jan 2026) | Google Search grounding |

**Usage**: Primary chat model (Studio). Tool-gated routing:
- **Flash (default)**: Simple queries, file ops, search, memory operations
- **Pro (escalated)**: When heavy tools called (goal, task, send_instruction) or chain depth > 3

### **6.2 Embeddings (Gemini)**

| Property | Value |
|----------|-------|
| Model | `gemini-embedding-001` |
| Dimensions | 3072 |
| Endpoint | `https://generativelanguage.googleapis.com/v1beta/models/gemini-embedding-001:embedContent` |
| Batch endpoint | `...gemini-embedding-001:batchEmbedContents` |
| Auth | API key in URL (`GEMINI_API_KEY`) |
| Cost | Free tier |
| Max batch | 50 items |
| Max text | 8000 chars |

**Why Gemini**: Free tier with good quality. Switched from OpenAI embeddings (2025-12-11) to eliminate costs.

### **6.3 Qdrant Vector Database**

| Property | Value |
|----------|-------|
| Protocol | gRPC |
| Default URL | `http://localhost:6334` |
| Client | `qdrant_client` crate |
| Distance | Cosine similarity |

#### **Collections**
| Collection | Purpose |
|------------|---------|
| `mira_code` | Code symbols, functions, classes |
| `mira_conversation` | Chat messages, summaries |
| `mira_docs` | Ingested documents (PDF, markdown) |

#### **Search Parameters**
| Constant | Value | Description |
|----------|-------|-------------|
| `SEMANTIC_SEARCH_MIN_SCORE` | 0.3 | Minimum similarity threshold |
| `SEMANTIC_SEARCH_DEFAULT_LIMIT` | 10 | Default results per query |
| `SEMANTIC_SEARCH_MAX_LIMIT` | 100 | Maximum results |

**Graceful degradation**: If Qdrant is unavailable, Mira falls back to SQL-based search (slower, less accurate but functional).

### **6.4 MCP Protocol (Claude Code)**

| Property | Value |
|----------|-------|
| Crate | `rmcp` |
| Transport | `StreamableHttpService` |
| Session manager | `LocalSessionManager` |
| Default config | `StreamableHttpServerConfig::default()` |

#### **MCP Architecture**
```
Claude Code → mira connect (stdio) → HTTP POST → Daemon → MiraServer
                                                    ↓
                                              Tool Router
                                                    ↓
                                              core::ops/*
```

**Key files**:
- `src/main.rs`: MCP service setup with `StreamableHttpService`
- `src/tools/mod.rs`: Tool router with 30+ registered tools
- `src/connect.rs`: Stdio-to-HTTP bridge for `mira connect`

#### **Registered MCP Tools**
| Category | Tools |
|----------|-------|
| Memory | `remember`, `recall`, `forget` |
| Session | `session_start`, `get_session_context`, `store_session`, `search_sessions` |
| Tasks | `task`, `goal`, `proposal` |
| Corrections | `correction`, `store_decision`, `record_rejected_approach` |
| Code Intel | `get_symbols`, `get_call_graph`, `semantic_code_search`, `get_related_files`, `find_cochange_patterns` |
| Git Intel | `get_recent_commits`, `search_commits` |
| Build | `build`, `find_similar_fixes`, `record_error_fix` |
| Documents | `document` (list/search/get/ingest/delete) |
| Index | `index` (project/file/status/cleanup) |
| Context | `carousel`, `get_proactive_context`, `get_work_state`, `sync_work_state` |
| Batch | `batch` (create/list/get/cancel) - 50% cost savings for async ops |
| File Search | `file_search` (index/list/remove/status) - per-project RAG |
| Admin | `get_project`, `set_project`, `permission`, `get_guidelines`, `add_guideline`, `query`, `list_tables` |

### **6.5 Gemini Built-in Tools**

Gemini 3 provides built-in capabilities that are automatically enabled:

| Tool | Description | Cost |
|------|-------------|------|
| `google_search` | Real-time web search with source citations | FREE until Jan 2026 |
| `code_execution` | Python sandbox (40+ libraries, 30s timeout) | Included |
| `url_context` | Native web page fetching (20 URLs, 34MB each) | Included |

**Grounding Metadata**: Search results include `groundingChunks` with source URIs and titles, automatically cited in responses.

**Code Execution**: Returns `executableCode` (Python) and `codeExecutionResult` (output/outcome). Supports matplotlib, numpy, pandas, sklearn.

**Context Caching**: Gemini supports caching system prompts and context for ~75% cost reduction:
- Flash: minimum 1,024 tokens
- Pro: minimum 4,096 tokens
- TTL: configurable (default 1 hour)

### **6.6 External API Patterns**

All external calls follow consistent patterns:

```rust
// Timeout handling
let client = reqwest::Client::builder()
    .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))  // 30s default
    .build()?;

// Retry with backoff
for attempt in 0..EMBED_RETRY_ATTEMPTS {  // 2 attempts
    match api_call().await {
        Ok(result) => return Ok(result),
        Err(e) if attempt < EMBED_RETRY_ATTEMPTS - 1 => {
            tokio::time::sleep(Duration::from_millis(RETRY_DELAY_MS)).await;  // 500ms
        }
        Err(e) => return Err(e),
    }
}

// Graceful degradation
if !semantic.is_available() {
    // Fall back to SQL search
}
```

**Error handling**: All external calls return `Result<T>` with descriptive errors. Transient failures retry; permanent failures degrade gracefully.

## 7. **Operational Characteristics**

### **7.1 Resource Usage**

#### **SQLite Connection Pool**
| Parameter | Value | Rationale |
|-----------|-------|-----------|
| `max_connections` | 10 | SQLite is single-writer; more readers than writers |
| `min_connections` | 2 | Keep connections warm |
| `acquire_timeout` | 10s | Fail fast if pool exhausted |
| `max_lifetime` | 30 min | Recycle to prevent stale connections |
| `idle_timeout` | 10 min | Release unused connections |

#### **Memory Footprint**
| Component | Typical Usage |
|-----------|---------------|
| Base daemon | ~30-50 MB |
| Per-project watcher | ~5-10 MB |
| Qdrant client | ~10 MB |
| HTTP client pool | ~5 MB |
| File cache (30s TTL) | Up to 50 MB (100 files × 512KB max) |

#### **Concurrency Controls**
| Control | Value | Purpose |
|---------|-------|---------|
| `sync_semaphore` | 3 permits | Limit concurrent `/api/chat/sync` requests |
| `ProjectLocks` | Per-project mutex | Prevent concurrent writes to same project |
| `RwLock` on `active_project` | Reader-writer | Allow concurrent reads, exclusive writes |
| `RwLock` on `carousel` | Reader-writer | Context carousel state |

### **7.2 Background Tasks**

The daemon spawns several background tasks:

#### **File Watcher (per project)**
- Uses `notify` crate for filesystem events
- Debounces rapid changes
- Re-indexes modified files automatically
- Handles file deletions (removes from index)

#### **Git Sync (every 5 minutes)**
- Fetches from origin
- Fast-forward pulls if possible
- Re-indexes changed files
- Updates commit history and cochange patterns

#### **Initial Index (on startup)**
- Runs if no symbols exist for a project
- Indexes all code files (AST parsing)
- Indexes git history (500 commits)
- Generates embeddings for Qdrant

### **7.3 Performance Characteristics**

#### **Latency Targets**
| Operation | Target | Notes |
|-----------|--------|-------|
| Health check | <10ms | Simple status JSON |
| Memory recall | <100ms | Qdrant query + DB lookup |
| Code search | <200ms | Embedding + vector search |
| Tool execution (simple) | <50ms | DB read/write |
| Chat streaming | First token <2s | Gemini 3 Flash/Pro |

#### **Throughput**
| Metric | Value |
|--------|-------|
| Concurrent MCP sessions | Unlimited (each is stateless) |
| Concurrent chat streams | Limited by sync_semaphore (3) |
| SQLite writes | ~1000/s (single-threaded) |
| Qdrant inserts | ~100/s (embedding generation is bottleneck) |

### **7.4 Failure Modes & Recovery**

#### **External Service Failures**

| Service | Failure Mode | Recovery Strategy |
|---------|--------------|-------------------|
| Qdrant down | Semantic search unavailable | Fall back to SQL LIKE queries |
| Gemini API error | Embedding/chat fails | Retry 2x with 500ms backoff, return error |
| DeepSeek timeout | Chat stalls | 180s timeout, return error to user |

#### **Database Failures**

| Failure | Detection | Recovery |
|---------|-----------|----------|
| Connection pool exhausted | `acquire_timeout` (10s) | Return 503, client retries |
| Migration failure | Startup check | Daemon refuses to start, logs error |
| Corrupted database | Query errors | Manual restore from backup |

#### **Process Failures**

| Failure | Detection | Recovery |
|---------|-----------|----------|
| Daemon crash | systemd watchdog | Auto-restart after 5s (`Restart=always`) |
| OOM kill | systemd journal | Restart, investigate memory leak |
| File watcher crash | Task panic | Other watchers unaffected, manual restart |

### **7.5 Health Endpoint**

`GET /health` returns component status:

```json
{
  "status": "ok",           // or "degraded"
  "database": "ok",         // or "error"
  "semantic_search": "ok",  // or "unavailable" or "error"
  "version": "2.0.0"
}
```

**Status meanings**:
- `ok`: All systems operational
- `degraded`: Core functionality works, some features disabled (e.g., no Qdrant)
- `error`: Critical failure, investigate immediately

### **7.6 Observability**

#### **Logging**
- Framework: `tracing` crate with `FmtSubscriber`
- Output: systemd journal (`journalctl --user -u mira`)
- Levels: `RUST_LOG=info` (default), `debug` for verbose

#### **Key Log Events**
| Event | Level | Example |
|-------|-------|---------|
| Startup | INFO | `Starting Mira Daemon on 127.0.0.1:3000...` |
| DB connect | INFO | `Database connected: sqlite://...` |
| Index complete | INFO | `Initial index complete: 1234 symbols` |
| Git sync | INFO | `Git sync: 5 commits, 12 cochange patterns` |
| API error | WARN | `DeepSeek API error: rate limited` |
| Auth failure | WARN | `Unauthorized request to /api/chat/sync` |
| Fatal error | ERROR | `Migration failed: ...` |

#### **Metrics (via /health)**
- Database connectivity
- Semantic search availability
- Schema version (migration count)

### **7.7 Scaling Considerations**

Mira is designed as a **single-user, single-machine** tool. Scaling horizontally is not a goal.

#### **Vertical Scaling**
| Resource | Impact |
|----------|--------|
| More RAM | Larger file cache, more concurrent operations |
| Faster SSD | Faster SQLite, faster Qdrant |
| More CPU | Faster embedding generation, faster indexing |

#### **Limits**
| Resource | Practical Limit | Bottleneck |
|----------|-----------------|------------|
| Projects watched | ~10 | File watcher memory |
| Code symbols indexed | ~100K | SQLite query time |
| Memories stored | ~50K | Qdrant search time |
| Concurrent users | 1 | By design (personal assistant) |

#### **What Won't Scale**
- Multiple users (auth is single-token)
- Multiple machines (SQLite is local)
- Very large codebases (>1M LoC) without tuning

## 8. **Security & Compliance**

> **Note**: Mira is designed as a personal tool, not a multi-tenant service. Security is oriented toward protecting the single user's data, not isolation between users.

### **8.1 Authentication**

#### **Token-Based Auth**
| Property | Value |
|----------|-------|
| Token format | UUID v4 (36 chars) |
| Storage | `~/.mira/token` (file permissions: 0600) |
| Generation | Auto-generated on first run |
| Headers | `Authorization: Bearer <token>` or `X-Auth-Token: <token>` |

#### **Auth Middleware Behavior**
| Bind Address | `/health` | `/api/*` | `/mcp` |
|--------------|-----------|----------|--------|
| `127.0.0.1` (localhost) | Public | **No auth** (trusted) | Requires auth |
| `0.0.0.0` (exposed) | Public | Requires auth | Requires auth |

**Rationale**: When bound to localhost only, `/api/*` endpoints skip auth for convenience. When exposed externally, everything except `/health` requires auth.

#### **Current Gaps** ⚠️
- No user accounts or role-based access
- No session expiry or token rotation
- No rate limiting
- mira.conarylabs.com currently has no auth protection

### **8.2 Secret Detection & Redaction**

Mira automatically detects and handles secrets in tool output.

#### **Detected Patterns**
| Category | Patterns |
|----------|----------|
| Private keys | RSA, EC, OpenSSH, PGP, generic |
| API keys | OpenAI (`sk-proj-`), Anthropic (`sk-ant-`), Google (`AIzaSy`) |
| GitHub | PAT (`ghp_`), OAuth (`gho_`), User (`ghu_`), Server (`ghs_`) |
| AWS | Access keys (`AKIA`), secret keys |
| Payment | Stripe (`sk_live_`, `sk_test_`), Twilio |
| Messaging | Slack (`xoxb-`, `xoxp-`), Discord |
| Generic | `bearer `, `token=`, `password=`, `API_KEY=` |

#### **Handling**
1. **Detection**: `detect_secrets()` scans output for patterns
2. **Flagging**: Artifacts marked with `contains_secrets: true`
3. **Redaction**: `redact_secrets()` replaces with `[REDACTED: kind]`
4. **TTL reduction**: Secret-containing artifacts expire in 24h (vs 7 days default)

```rust
// Example redaction
"token: sk-proj-abc123xyz789"
→ "token: [REDACTED: openai_key]"
```

### **8.3 Data Protection**

#### **At Rest**
| Data | Protection |
|------|------------|
| SQLite database | File permissions (user-only) |
| API keys in .env | File permissions, not in git |
| Auth token | `~/.mira/token` with 0600 permissions |
| Qdrant vectors | Local only, no auth (single-user assumption) |

#### **In Transit**
| Path | Protection |
|------|------------|
| Localhost connections | Unencrypted (trusted network) |
| External API calls | HTTPS/TLS |
| mira.conarylabs.com | Should use HTTPS (nginx) |

#### **Artifact Security**
| Feature | Implementation |
|---------|----------------|
| Auto-expiry | TTL per artifact type (7d/30d/24h for secrets) |
| Secret detection | Pattern matching before storage |
| Size limits | 10MB max artifact, 64KB sync messages |

### **8.4 Systemd Hardening**

The daemon runs with restricted capabilities:

```ini
# Security hardening in mira.service
NoNewPrivileges=true      # Prevent privilege escalation
ProtectSystem=strict      # Read-only /usr, /boot, /etc
ProtectHome=read-only     # Read-only home except...
ReadWritePaths=%h/.mira %h/Mira/data  # ...these paths
PrivateTmp=true           # Isolated /tmp
```

### **8.5 Input Validation**

| Layer | Validation |
|-------|------------|
| MCP | Schema validation via `rmcp` |
| HTTP | Axum extractors, content-length limits |
| SQL | Parameterized queries (sqlx), no raw SQL |
| File paths | Canonicalization, project-scoped access |

### **8.6 Audit Logging**

Currently minimal:

| What's Logged | Where |
|---------------|-------|
| Auth failures | journalctl (WARN) |
| API errors | journalctl (WARN/ERROR) |
| Tool calls | Not logged (privacy) |
| Database queries | Not logged |

#### **Future Considerations**
- MCP call history table exists (`mcp_tool_calls`) but not fully utilized
- Could add opt-in audit mode for debugging
- No GDPR/compliance features currently

### **8.7 Threat Model**

| Threat | Mitigation | Status |
|--------|------------|--------|
| Unauthorized access | Token auth | ✅ (localhost), ⚠️ (exposed) |
| Secret leakage | Detection + redaction | ✅ |
| SQL injection | Parameterized queries | ✅ |
| Path traversal | Canonicalization | ✅ |
| Privilege escalation | systemd NoNewPrivileges | ✅ |
| Man-in-the-middle | HTTPS for external APIs | ✅ |
| Local access by other users | File permissions | ✅ |
| Remote attacks on public instance | Rate limiting, WAF | ❌ Not implemented |

### **8.8 Recommendations for Production**

If deploying Mira externally:

1. **Always use HTTPS** via nginx/Cloudflare
2. **Rotate auth token** periodically
3. **Add rate limiting** at nginx level
4. **Enable UFW** to restrict ports
5. **Monitor logs** for auth failures
6. **Consider Cloudflare Access** for additional auth layer
7. **Don't expose** unless necessary (use SSH tunnels instead)

## 9. **Development Workflow**

### **9.1 Project Structure**

```
/home/peter/Mira/
├── src/                    # Rust source code
│   ├── main.rs             # CLI entry point
│   ├── lib.rs              # Library root
│   ├── batch/              # Batch processing (Gemini Batch API)
│   ├── chat/               # Studio chat backend
│   ├── context/            # Carousel, context assembly
│   ├── core/               # Shared ops, primitives
│   ├── daemon.rs           # Background tasks
│   ├── hooks/              # Claude Code hooks
│   ├── indexer/            # Code/git indexing
│   ├── server/             # MCP server, DB pool
│   └── tools/              # MCP tool implementations
├── studio/                 # SvelteKit frontend
│   ├── src/                # Svelte components
│   └── static/             # Static assets
├── migrations/             # SQLx migrations (consolidated)
├── tests/                  # Integration tests
├── docs/                   # Documentation
├── data/                   # SQLite database (symlinked)
└── .sqlx/                  # Offline query cache
```

### **9.2 Build System**

#### **Rust Backend**
| Property | Value |
|----------|-------|
| Edition | 2024 |
| Rust version | 1.92+ |
| Package version | 2.0.0 |
| Offline mode | `SQLX_OFFLINE=true` (queries pre-checked) |

```bash
# Development build
cargo build

# Release build (optimized)
SQLX_OFFLINE=true cargo build --release

# Check without building
cargo check

# Clippy lints
cargo clippy
```

#### **Studio Frontend**
| Property | Value |
|----------|-------|
| Framework | SvelteKit 2 + Svelte 5 |
| Build tool | Vite 6 |
| CSS | Tailwind 4 |
| Adapter | Static (pre-rendered) |

```bash
cd studio
npm run dev      # Development server
npm run build    # Production build
npm run preview  # Preview production build
```

**Key UX Components** (as of 2025-12-25):
| Component | Purpose |
|-----------|---------|
| `NavRail.svelte` | Left nav with enum state (`collapsed`/`expanded`/`settings`) |
| `StreamingStatus.svelte` | Live streaming status with tool names and token counts |
| `ProjectSelector.svelte` | Project cards with pin/unpin and last activity |
| `ToolArguments.svelte` | Structured key-value argument display (not raw JSON) |
| `TerminalView.svelte` | Messages with `[you]`/`[mira]` role labels and hover timestamps |

#### **Full Rebuild Script**
```bash
./rebuild.sh                 # Rebuild everything, restart services
./rebuild.sh --backend-only  # Just Mira backend
./rebuild.sh --frontend-only # Just Studio frontend
./rebuild.sh --no-restart    # Build without restarting
```

### **9.3 Testing**

#### **Test Counts**
| Category | Count |
|----------|-------|
| Unit tests (`#[test]`) | ~180 |
| Integration test files | 4 |
| SQLx migrations | 1 (consolidated) |

#### **Test Suites**
| File | Purpose |
|------|---------|
| `tests/daemon_e2e.rs` | Daemon startup, background tasks |
| `tests/integration_e2e.rs` | Full API integration |
| `tests/mira_core_contract.rs` | Core ops behavior contracts |
| `tests/tool_parity.rs` | MCP ↔ Chat tool equivalence |

#### **Running Tests**
```bash
# All tests
cargo test

# Specific test
cargo test test_remember

# With output
cargo test -- --nocapture

# Integration tests only
cargo test --test integration_e2e
```

### **9.4 Database Migrations**

SQLx handles schema migrations automatically:

```bash
# Create new migration
sqlx migrate add <name>

# Run pending migrations (happens on daemon start)
sqlx migrate run

# Revert last migration
sqlx migrate revert

# Check migration status
sqlx migrate info
```

#### **Offline Mode**
Queries are pre-checked at compile time via `.sqlx/` cache:

```bash
# Regenerate offline cache (requires running DB)
cargo sqlx prepare

# Build with offline mode (no DB needed)
SQLX_OFFLINE=true cargo build
```

### **9.5 Development Commands**

#### **Quick Reference**
```bash
# Start daemon (foreground, for debugging)
cargo run

# Start daemon (background, via systemd)
systemctl --user start mira

# Watch logs
journalctl --user -u mira -f

# Check status
mira status

# Connect via MCP (test)
mira connect

# Run with debug logging
RUST_LOG=debug cargo run
```

### **9.6 Code Organization Conventions**

#### **Module Structure**
- `mod.rs` exports public API
- `types.rs` for data structures
- One file per logical component
- Tests in same file (`#[cfg(test)]` module)

#### **Error Handling**
- Use `anyhow::Result` for fallible functions
- Use `thiserror` for custom error types in `core/error.rs`
- Prefer `.context()` over `.unwrap()` for better error messages

#### **Async Patterns**
- Tokio runtime throughout
- `Arc<T>` for shared state
- `RwLock` for read-heavy shared state
- `Mutex` for write-heavy or simple cases
- `spawn_blocking` for CPU-bound work (git2, parsing)

### **9.7 Deployment**

#### **Local Development**
1. Clone repo
2. Copy `.env.example` to `.env`, fill in API keys
3. `cargo build --release`
4. `./rebuild.sh`

#### **Production (VPS)**
1. SSH to server
2. `git pull`
3. `./rebuild.sh`
4. Verify: `mira status`, `curl localhost:3000/health`

#### **Install Script**
```bash
# Fresh install (uses Docker)
curl -fsSL https://raw.githubusercontent.com/ConaryLabs/Mira/main/install.sh | bash
```

### **9.8 Documentation**

| File | Purpose |
|------|---------|
| `README.md` | Project overview, quick start |
| `CLAUDE.md` | Instructions for Claude Code |
| `docs/ARCHITECTURE.md` | This document |
| `MCP_DEBUG_STATUS.md` | Debugging notes |

### **9.9 No CI/CD (Yet)**

Currently no automated CI/CD pipeline. Deployment is manual:

1. Develop locally
2. Test with `cargo test`
3. Push to GitHub
4. SSH to VPS, `git pull && ./rebuild.sh`

#### **Future Considerations**
- GitHub Actions for `cargo test` on PR
- Automated deployment on push to main
- Docker image publishing

---

## **Document Statistics**

| Metric | Value |
|--------|-------|
| Document lines | ~1,595 |
| Major sections | 9 |
| Subsections | ~65 |
| Decision ledger entries | 17 (spanning 5 months) |
| Code references | ~50 file paths |

### **Codebase Counts**
| Component | Count |
|-----------|-------|
| Source files | ~90 |
| Core ops modules | 18 |
| MCP tools | 30+ |
| Chat tools | 17 + 8 tool_defs |
| Unit tests | ~180 |
| Integration tests | 4 |
| SQLx migrations | 1 (consolidated) |

---

### **Revision History**

| Date | Rev | Author | Changes |
|------|-----|--------|---------|
| 2025-12-25 | 1.0 | Peter + Claude | Initial comprehensive documentation. All 9 sections complete. |
| 2025-12-25 | 1.1 | Peter + Claude | Council review feedback: added Carousel state machine diagram, context composition strategy, Hotline interface clarification. |
| 2025-12-25 | 1.2 | Peter + Claude | Studio UX overhaul: enum-based layout state, message role labels, streaming status, tool argument rendering, project management UI. |
| 2025-12-26 | 1.3 | Peter + Claude | Gemini 3 maximization: FileSearch (RAG), Batch API (50% cost savings), thinking levels, cached token tracking, URL context metadata. |
| 2025-12-27 | 1.4 | Peter + Claude | Claude Code spawner module: process lifecycle, stream-json parsing, session endpoints, Studio integration, instruction-session linking. |

---

*This document is the authoritative reference for Mira's architecture. Keep it updated.*