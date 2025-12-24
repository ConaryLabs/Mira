# Mira Architecture

## Core Principle: Shared Business Logic

All business logic lives in `src/core/ops/`. Both MCP (the Claude Code integration) and Studio (the chat interface) use the same underlying operations.

```
┌─────────────────┐     ┌─────────────────┐
│   MCP Server    │     │  Studio/Chat    │
│  (src/tools/)   │     │  (src/chat/)    │
└────────┬────────┘     └────────┬────────┘
         │                       │
         │   Thin wrappers       │
         │                       │
         └───────────┬───────────┘
                     │
                     ▼
         ┌───────────────────────┐
         │     core::ops         │
         │  (Business Logic)     │
         └───────────┬───────────┘
                     │
                     ▼
         ┌───────────────────────┐
         │   core::primitives    │
         │  (Utilities/Helpers)  │
         └───────────────────────┘
```

## Directory Structure

```
src/
├── core/
│   ├── primitives/          # Utilities, helpers, shared types
│   │   ├── semantic.rs      # Qdrant + embedding search
│   │   ├── memory.rs        # Memory fact data structures
│   │   ├── artifacts.rs     # Large output storage
│   │   ├── secrets.rs       # Secret detection
│   │   ├── excerpts.rs      # Smart text extraction
│   │   ├── limits.rs        # Constants & thresholds
│   │   └── streaming.rs     # SSE decoding
│   │
│   ├── ops/                 # Business logic (SINGLE SOURCE OF TRUTH)
│   │   ├── memory.rs        # Remember, recall, forget
│   │   ├── mira.rs          # Tasks, goals, corrections, decisions
│   │   ├── file.rs          # Read, write, edit, glob, grep
│   │   ├── shell.rs         # Bash execution
│   │   ├── git.rs           # Git status, diff, commit, log, cochange
│   │   ├── web.rs           # Web search, fetch
│   │   ├── code_intel.rs    # Symbol lookup, call graphs, semantic search
│   │   ├── build.rs         # Build tracking, error fixes
│   │   ├── documents.rs     # Document management
│   │   ├── work_state.rs    # Session state persistence
│   │   ├── session.rs       # Session start, store, search
│   │   ├── chat_summary.rs  # Chat summarization (Studio)
│   │   └── chat_chain.rs    # Response chain, handoff (Studio)
│   │
│   ├── context.rs           # OpContext - dependency injection
│   └── mod.rs               # Re-exports
│
├── tools/                   # MCP tool wrappers (thin layer)
│   ├── memory.rs            # → core::ops::memory
│   ├── mira.rs              # → core::ops::mira
│   ├── code_intel.rs        # → core::ops::code_intel
│   ├── git_intel.rs         # → core::ops::git, core::ops::build
│   └── ...
│
├── chat/                    # Studio/Chat (thin layer)
│   ├── tools/               # Chat tool implementations
│   │   ├── memory.rs        # → core::ops::memory
│   │   ├── mira.rs          # → core::ops::mira
│   │   ├── file.rs          # → core::ops::file
│   │   └── ...
│   │
│   ├── session/             # Session management
│   │   ├── summarization.rs # → core::ops::chat_summary
│   │   ├── chain.rs         # → core::ops::chat_chain
│   │   └── ...
│   │
│
└── server/                  # MCP server entry point
```

## OpContext: Dependency Injection

All `core::ops` functions take `&OpContext` as their first parameter. This provides clean dependency injection without coupling to MCP or Chat specifics.

```rust
pub struct OpContext {
    pub db: Option<SqlitePool>,
    pub semantic: Option<Arc<SemanticSearch>>,
    pub http: reqwest::Client,
    pub cwd: PathBuf,
    pub project_path: String,
    pub cancel: CancellationToken,
}
```

### Construction Patterns

```rust
// Minimal context
let ctx = OpContext::new(cwd);

// With database
let ctx = OpContext::new(cwd).with_db(db);

// With semantic search
let ctx = OpContext::new(cwd)
    .with_db(db)
    .with_semantic(semantic);

// Convenience for DB-only ops
let ctx = OpContext::just_db(db);
```

## Adding New Operations

1. **Define types** in `core/ops/your_module.rs`:
   ```rust
   pub struct YourInput { ... }
   pub struct YourOutput { ... }
   ```

2. **Implement the operation**:
   ```rust
   pub async fn your_operation(ctx: &OpContext, input: YourInput) -> CoreResult<YourOutput> {
       let db = ctx.require_db()?;
       // ... business logic
   }
   ```

3. **Add thin wrapper** in `tools/` or `chat/`:
   ```rust
   pub async fn your_operation(&self, req: Request) -> Result<Response> {
       let ctx = OpContext::new(cwd).with_db(self.db.clone());
       let input = YourInput { ... };
       let output = core_ops::your_operation(&ctx, input).await?;
       Ok(Response { ... })
   }
   ```

## Why This Architecture?

1. **Single source of truth** - Business logic exists in one place
2. **Testability** - core::ops can be tested without MCP/Chat overhead
3. **Consistency** - Same behavior whether accessed via MCP or Studio
4. **Maintainability** - Changes to logic only need to happen once
5. **Flexibility** - Easy to add new interfaces (CLI, API, etc.)

## Design Decisions

### User-Facing Strings
`core::ops` does **not** construct user-facing display strings. All formatting for prompts, UI, or responses happens in the adapter layers (MCP tools, Chat session). The ops return structured data types.

Exceptions:
- Error messages in `CoreError` (technical, not user-facing)
- Metadata strings for semantic search storage

### Auth/Policy Enforcement
Authorization and policy checks happen in the **adapter layer**, not in `core::ops`. Operations assume they're being called by authorized code.

- MCP: Tool permissions managed via `permissions` tool in `server/mod.rs`
- Studio: Session-scoped (no cross-user access)

### OpContext Size
Current `OpContext` has 6 fields - intentionally minimal:
- `db: Option<SqlitePool>` - database access
- `semantic: Option<Arc<SemanticSearch>>` - vector search
- `http: reqwest::Client` - external API calls
- `cwd: PathBuf` - file operations root
- `project_path: String` - scoping
- `cancel: CancellationToken` - cancellation

If this grows beyond ~10 fields, consider splitting into capability traits (`HasDb`, `HasSemantic`, etc.).

## Chat Chain Invariants

The `chat_chain` module manages response chain state with these invariants:

### Reset Decision State Machine

```
                          ┌──────────────────┐
                          │   Every Turn     │
                          └────────┬─────────┘
                                   │
                    ┌──────────────▼───────────────┐
                    │ Check: turns_since_reset     │
                    │        < COOLDOWN (3)?       │
                    └──────────────┬───────────────┘
                            yes    │    no
                    ┌──────────────┴───────────────┐
                    ▼                              ▼
            ┌───────────┐              ┌───────────────────┐
            │ Cooldown  │              │ Check: tokens >   │
            │ (skip)    │              │ HARD_CEILING(420k)│
            └───────────┘              └─────────┬─────────┘
                                          yes    │    no
                                    ┌────────────┴────────────┐
                                    ▼                         ▼
                            ┌───────────┐        ┌────────────────────┐
                            │ HardReset │        │ Check: tokens >    │
                            └───────────┘        │ THRESHOLD(400k) && │
                                                 │ cache < 30%        │
                                                 └─────────┬──────────┘
                                                    yes    │    no
                                        ┌──────────────────┴─────────────┐
                                        ▼                                ▼
                              ┌─────────────────────┐          ┌─────────────────┐
                              │ consecutive_low++   │          │ consecutive_low │
                              │ Check: >= HYSTER(2) │          │ = 0             │
                              └─────────┬───────────┘          └─────────────────┘
                                 yes    │    no
                              ┌─────────┴─────────┐
                              ▼                   ▼
                      ┌───────────┐       ┌───────────┐
                      │ SoftReset │       │ NoReset   │
                      └───────────┘       └───────────┘
```

### Key Invariants

1. **Cooldown**: After any reset, wait `COOLDOWN_TURNS` (3) before considering another
2. **Hard ceiling**: If tokens > 420k, always reset (quality guard)
3. **Hysteresis**: Soft reset requires `HYSTERESIS_TURNS` (2) consecutive low-cache turns
4. **Cache threshold**: Low cache = below 30%
5. **Handoff preservation**: On soft reset, `build_handoff_blob()` captures context before clearing

### Handoff Blob Contents

When a soft reset occurs, the handoff blob captures:
1. Recent conversation (last 6 messages, truncated to 500 chars each)
2. Latest summary (older context)
3. Active goals (up to 3)
4. Recent decisions (up to 5)
5. Working set (touched files, up to 10)
6. Last failure (if any)
7. Recent artifacts (up to 5)
8. Continuity note

### State Variables

| Variable | Purpose | Reset on |
|----------|---------|----------|
| `consecutive_low_cache_turns` | Hysteresis counter | Reset or good cache turn |
| `turns_since_reset` | Cooldown tracking | Every reset |
| `needs_handoff` | Flag for next turn | After consumption |
| `handoff_blob` | Context for next turn | After consumption |

## Testing

The test suite is organized by scope:

```
tests/
├── daemon_e2e.rs       # MCP daemon tool E2E tests (25 tests)
├── integration_e2e.rs  # Chat server API E2E tests (10 tests)
└── mira_core_contract.rs # Core primitive contracts (11 tests)
```

### daemon_e2e.rs

Tests all MCP tools by calling the underlying `src/tools/` functions directly with an isolated test database. Covers:

- **Project/Session**: set_project, session_start, get_session_context
- **Memory**: remember, recall, forget (with upsert semantics)
- **Tasks**: create, list, get, update, complete, delete, subtasks
- **Goals**: create, list, update, milestones, progress tracking
- **Corrections**: record, get, validate workflow
- **Build tracking**: record_build, record_error, get_errors, resolve
- **Permissions**: save, list, delete rules
- **Analytics**: list_tables, read-only queries
- **Guidelines**: add, get coding guidelines
- **Work state**: sync, get for session resume
- **Sessions**: store_decision, store_session, search_sessions
- **Code intel**: get_symbols, get_call_graph, semantic_code_search
- **Git intel**: get_commits, search_commits, cochange_patterns, record_error_fix
- **Documents**: list, get, delete
- **Proactive context**: assembled context for tasks
- **MCP history**: log_call, search_history

### integration_e2e.rs

Tests the Chat/Studio HTTP API using axum's test utilities (no server spawn). Covers:

- Status endpoint
- Message pagination and filtering
- Sync endpoint validation and auth
- Payload size limits
- Concurrent request handling (semaphore)
- Project locks (per-project serialization)
- Archived message exclusion

### mira_core_contract.rs

Tests core primitive contracts (SemanticSearch, MetadataBuilder, embedding config).

### Test Patterns

```rust
// Create isolated test database with migrations
async fn create_test_db(temp_dir: &TempDir) -> SqlitePool {
    let db_path = temp_dir.path().join("test.db");
    let pool = create_optimized_pool(&format!("sqlite://{}?mode=rwc", db_path.display())).await?;
    run_migrations(&pool, &migrations_path).await?;
    pool
}

// SemanticSearch disabled for unit tests (no Qdrant dependency)
async fn create_test_semantic() -> Arc<SemanticSearch> {
    Arc::new(SemanticSearch::new(None, None).await)
}

// Use temp_dir path as project_path (passes validation)
fn get_project_path(temp_dir: &TempDir) -> String {
    temp_dir.path().to_string_lossy().to_string()
}
```

## Database Tables

The database schema is defined in `migrations/`. Key tables:

| Table | Purpose |
|-------|---------|
| `projects` | Project metadata and paths |
| `memory_facts` | Semantic memory (remember/recall) |
| `memory_entries` | Session summaries |
| `tasks` | Task tracking |
| `goals` | Goal/milestone tracking |
| `corrections` | User corrections for learning |
| `coding_guidelines` | Per-project coding standards |
| `code_symbols` | Indexed code symbols |
| `git_commits` | Indexed git history |
| `cochange_patterns` | Files that change together |
| `error_fixes` | Learned error solutions |
| `rejected_approaches` | Approaches to avoid |
| `chat_messages` | Studio conversation history |
| `chat_summaries` | Studio context compression |
| `chat_context` | Studio session state |
| `advisory_sessions` | Multi-turn advisory conversations |
| `advisory_messages` | Advisory conversation history |
| `advisory_pins` | Pinned constraints in advisory sessions |
| `advisory_decisions` | Decisions made in advisory sessions |
| `advisory_summaries` | Compressed older turns in sessions |

## Advisory Module

The `src/advisory/` module provides a unified abstraction for consulting external LLMs.

### Architecture

```
src/advisory/
├── mod.rs           # AdvisoryService - main entry point
├── provider.rs      # AdvisoryProvider trait + implementations
├── session.rs       # Multi-turn sessions with tiered memory
├── synthesis.rs     # Structured synthesis with provenance
├── streaming.rs     # SSE parsing for all providers
└── tool_bridge.rs   # Agentic tool calling with budget governance
```

### AdvisoryService

Single entry point for all advisory functionality:

```rust
let service = AdvisoryService::from_env()?;

// Single model query
let response = service.ask(AdvisoryModel::Gpt52, "question").await?;

// Council query (multiple models + synthesis)
let council = service.council("question", Some(AdvisoryModel::Opus45)).await?;
```

### Providers

| Model | Provider | Role |
|-------|----------|------|
| GPT-5.2 | OpenAI | Council member, reasoning |
| Opus 4.5 | Anthropic | Council member, extended thinking |
| Gemini 3 Pro | Google | Council member, thinking mode |
| DeepSeek Reasoner | DeepSeek | Synthesizer (not in council) |

### Council Flow

```
┌─────────┐  ┌─────────┐  ┌─────────┐
│ GPT-5.2 │  │ Opus4.5 │  │ Gemini  │   ← Council members (parallel)
└────┬────┘  └────┬────┘  └────┬────┘
     │            │            │
     └────────────┼────────────┘
                  │
                  ▼
         ┌────────────────┐
         │ DeepSeek       │   ← Synthesizer
         │ Reasoner       │
         └────────┬───────┘
                  │
                  ▼
         ┌────────────────┐
         │ CouncilSynthesis│  ← Structured output
         │ - consensus     │     with provenance
         │ - disagreements │
         │ - unique_insights│
         │ - recommendation│
         └────────────────┘
```

### Multi-Turn Sessions

Sessions support tiered memory for long conversations:

1. **Recent turns** (verbatim) - last 6-12 messages
2. **Summaries** - compressed older turns
3. **Pins** - explicit constraints/requirements
4. **Decisions** - what was decided and why

### Agentic Tool Calling

External LLMs can call read-only Mira tools via `tool_bridge.rs`:

**Allowed tools** (9 read-only):
- `recall`, `get_corrections`, `get_goals`
- `semantic_code_search`, `get_symbols`, `get_related_files`
- `find_similar_fixes`, `get_recent_commits`, `search_commits`

**Security features**:
- Whitelist enforcement
- Budget governance (3 per-call, 10 per-session)
- Query cooldown (same fingerprint blocked for 3 turns)
- Loop prevention (hotline/council calls blocked)
- Output wrapped as untrusted data
