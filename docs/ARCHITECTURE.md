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
│   └── conductor/           # DeepSeek orchestration
│       ├── mira_intel.rs    # → core::ops::{build, git, mira}
│       └── ...
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
