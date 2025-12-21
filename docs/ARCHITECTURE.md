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
