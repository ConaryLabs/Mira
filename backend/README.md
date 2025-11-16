# Mira Backend

**AI-powered coding assistant backend built in Rust**

Mira orchestrates specialized LLMs (GPT-5 for reasoning, DeepSeek for code generation) with comprehensive memory systems, real-time WebSocket streaming, and intelligent context gathering. Built for performance, type safety, and maintainability.

---

## Quick Start

### Prerequisites

- **Rust 1.75+** (`rustup`)
- **SQLite 3.35+**
- **Qdrant** (vector database) running on `localhost:6333`
- **API Keys**: OpenAI (GPT-5 + embeddings), DeepSeek

### Installation

```bash
# Clone the repo
git clone <repo-url>
cd mira-backend

# Install dependencies
cargo build

# Set up environment
cp .env.example .env
# Edit .env with your API keys

# Run migrations
sqlx migrate run

# Start the server
cargo run
```

Server starts on `ws://localhost:3001/ws`

---

## Architecture Overview

### High-Level Design

```
┌─────────────────────────────────────────────────┐
│         WebSocket Layer (Bidirectional)         │
└───────────────────┬─────────────────────────────┘
                    │
┌───────────────────▼─────────────────────────────────┐
│         Unified Chat Handler (Router)            │
│  • Simple chat → GPT-5 direct                   │
│  • Complex ops → Operation Engine               │
└───────────────────┬─────────────────────────────────┘
                    │
        ┌───────────┴───────────┐
        │                       │
┌───────▼────────┐     ┌───────▼────────┐
│  GPT-5         │     │  DeepSeek      │
│  (Reasoning)   │────►│  (Code Gen)    │
└────────┬───────┘     └───────┬────────┘
         │                     │
         └──────────┬──────────┘
                    │
┌───────────────────▼─────────────────────────────────┐
│              Storage Layer                       │
│  • SQLite (structured data)                     │
│  • Qdrant (vector embeddings)                   │
│  • Git (code context)                           │
└─────────────────────────────────────────────────────┘
```

### Key Components

- **Operation Engine** (`src/operations/engine/`) - Modular orchestration of GPT-5 → DeepSeek workflows
  - `orchestration.rs` - Main execution logic with planning mode and dynamic reasoning
  - `lifecycle.rs` - State management (pending → started → delegating → completed)
  - `artifacts.rs` - Code artifact handling
  - `delegation.rs` - DeepSeek delegation with rich context
  - `context.rs` - Context gathering (memory + code + relationships)
  - `events.rs` - Event emissions for real-time updates (including plan/task events)
  - `simple_mode.rs` - Fast path for simple requests with low reasoning
  - `tasks/` - Task tracking system with WebSocket event streaming

- **Memory Systems** (`src/memory/`) - Hybrid storage (recent + semantic)
  - `service/` - MemoryService coordinating all memory operations
  - `storage/sqlite/` - Message storage with analysis
  - `storage/qdrant/` - Multi-head vector embeddings (5 heads: semantic, code, summary, documents, relationship)
  - `features/recall_engine/` - Hybrid search (recency + similarity)
  - `features/code_intelligence/` - File trees, function extraction, semantic code search
  - `features/message_pipeline/` - Message analysis → embedding → storage

- **LLM Providers** (`src/llm/provider/`)
  - `gpt5.rs` - GPT-5 via Responses API (streaming + tools)
  - `deepseek.rs` - DeepSeek Reasoner (code generation)
  - `openai_embeddings.rs` - text-embedding-3-large (3072 dimensions)

- **WebSocket Protocol** (`src/api/ws/`)
  - Two coexisting protocols: legacy chat + operations
  - Real-time streaming with cancellation support
  - Event-driven artifact delivery

---

## Key Concepts

### 1. LLM Orchestration Strategy

Mira uses **capability-based delegation**:

- **GPT-5** handles conversation, analysis, planning, and decision-making
- **DeepSeek** handles actual code generation when GPT-5 delegates via tool calls
- Each model plays to its strengths rather than being treated as interchangeable

### 2. Operation Lifecycle

```
                    ┌──── Simple (score > 0.7) ────┐
                    │                              │
PENDING → STARTED ──┤                              ├──→ DELEGATING → GENERATING → COMPLETED
                    │                              │                               ↓
                    └── Complex (score ≤ 0.7) ───┐│                           FAILED
                                                  ││
                                          PLANNING ┘
                                              ↓
                                        Task Tracking
```

Operations are complex workflows tracked through state transitions:

**Simple Operations (simplicity score > 0.7):**
1. User request → Operation created (PENDING)
2. Execution begins → STARTED
3. Direct execution with low reasoning (no planning)
4. Artifacts captured → COMPLETED

**Complex Operations (simplicity score ≤ 0.7):**
1. User request → Operation created (PENDING)
2. Execution begins → STARTED
3. **Planning phase** → GPT-5 generates execution plan with HIGH reasoning
4. **Plan parsed** → Tasks created and tracked in database
5. Execution with tools → DELEGATING
6. DeepSeek generates code → GENERATING
7. Tasks updated in real-time → COMPLETED

All state changes emit events via channels for real-time frontend updates, including:
- PlanGenerated (plan text + reasoning tokens)
- TaskCreated, TaskStarted, TaskCompleted, TaskFailed

### 3. Memory Architecture

**Three-tier system:**

1. **Recent Memory** - Last N messages (chronological)
2. **Semantic Memory** - Qdrant vector search across 5 heads
3. **Rolling Summaries** - Compressed context (10-message and 100-message windows)

**Embedding Heads:**
- `semantic` - General conversation
- `code` - Programming-related content
- `summary` - Conversation summaries
- `documents` - Project documentation
- `relationship` - User preferences, patterns, facts

### 4. Context Gathering Pipeline

Before each LLM call, Mira assembles:
- Recent messages (last 5-10)
- Semantic search results (top 10 similar messages)
- Rolling summaries (10 & 100 message windows)
- User relationships (preferences, facts)
- File tree (if project selected)
- Code intelligence (relevant functions/classes)

All packaged into a system prompt for maximum relevance.

---

## Project Structure

```
mira-backend/
├── Cargo.toml                     # Dependencies & metadata
├── Cargo.lock                     # Locked dependency versions
├── docker-compose.yaml            # Docker deployment config
├── .env.example                   # Environment template
├── .gitignore                     # Git exclusions
├── migrations/
│   └── 20251016_unified_baseline.sql  # Unified database schema
├── scripts/
│   └── reset_embeddings.sh        # Embedding maintenance script
├── test_runner.sh                 # Test execution script
├── src/
│   ├── main.rs                    # Server entry point
│   ├── lib.rs                     # Library root
│   ├── state.rs                   # AppState (DI container)
│   ├── utils.rs                   # Utility functions
│   ├── config/
│   │   └── mod.rs                 # Environment configuration
│   ├── api/
│   │   ├── error.rs               # API error types
│   │   ├── types.rs               # API request/response types
│   │   └── ws/                    # WebSocket handlers
│   │       ├── chat/              # Chat-specific handlers
│   │       │   ├── unified_handler.rs     # Main routing logic
│   │       │   ├── message_router.rs      # Message classification
│   │       │   ├── routing.rs             # Route determination
│   │       │   ├── connection.rs          # Connection management
│   │       │   └── heartbeat.rs           # Keepalive handling
│   │       ├── operations/        # Operation streaming
│   │       ├── code_intelligence.rs       # Code search endpoints
│   │       ├── documents.rs       # Document management
│   │       ├── files.rs           # File operations
│   │       ├── filesystem.rs      # FS navigation
│   │       ├── git.rs             # Git operations
│   │       ├── memory.rs          # Memory queries
│   │       ├── project.rs         # Project management
│   │       └── message.rs         # Message types
│   ├── operations/                # Operation engine
│   │   ├── engine/                # Modular orchestration
│   │   │   ├── orchestration.rs   # Main execution loop with planning
│   │   │   ├── lifecycle.rs       # State management
│   │   │   ├── artifacts.rs       # Artifact handling
│   │   │   ├── delegation.rs      # DeepSeek delegation
│   │   │   ├── context.rs         # Context gathering
│   │   │   ├── events.rs          # Event types (including plan/task events)
│   │   │   └── simple_mode.rs     # Fast path for simple requests
│   │   ├── tasks/                 # Task tracking system
│   │   │   ├── types.rs           # TaskStatus, OperationTask
│   │   │   ├── store.rs           # Database operations
│   │   │   └── mod.rs             # TaskManager
│   │   ├── types.rs               # Operation, Artifact types
│   │   └── delegation_tools.rs    # GPT-5 tool schemas
│   ├── memory/                    # Memory systems
│   │   ├── core/                  # Core abstractions
│   │   │   ├── types.rs           # MemoryEntry, etc.
│   │   │   ├── traits.rs          # Storage traits
│   │   │   └── config.rs          # Memory configuration
│   │   ├── service/               # Coordinated services
│   │   │   ├── core_service.rs    # Main MemoryService
│   │   │   ├── message_pipeline/  # Pipeline coordination
│   │   │   ├── recall_engine/     # Recall coordination
│   │   │   └── summarization_engine/  # Summary coordination
│   │   └── features/              # Specialized features
│   │       ├── message_pipeline/  # Analysis → Storage
│   │       │   └── analyzers/     # Chat, unified analyzers
│   │       ├── recall_engine/     # Hybrid search
│   │       │   ├── search/        # Search strategies
│   │       │   ├── scoring/       # Result scoring
│   │       │   └── context/       # Context building
│   │       ├── code_intelligence/ # Code parsing & search
│   │       ├── document_processing/  # Doc chunking
│   │       ├── summarization/     # Summary generation
│   │       ├── embedding.rs       # Embedding generation
│   │       ├── salience.rs        # Salience scoring
│   │       ├── decay.rs           # Memory decay
│   │       └── session.rs         # Session management
│   ├── llm/                       # LLM providers
│   │   ├── provider/
│   │   │   ├── gpt5.rs            # GPT-5 (Responses API)
│   │   │   ├── deepseek.rs        # DeepSeek Reasoner
│   │   │   ├── openai.rs          # OpenAI client
│   │   │   └── stream.rs          # Streaming utilities
│   │   ├── structured/            # Structured outputs
│   │   │   ├── tool_schema.rs     # Tool definitions
│   │   │   ├── processor.rs       # Response processing
│   │   │   ├── validator.rs       # Schema validation
│   │   │   └── types.rs           # Structured types
│   │   ├── embeddings.rs          # Embedding client
│   │   ├── reasoning_config.rs    # Reasoning parameters
│   │   └── types.rs               # Common LLM types
│   ├── relationship/              # User context
│   │   ├── service.rs             # RelationshipService
│   │   ├── facts_service.rs       # Fact storage
│   │   ├── storage.rs             # Persistence layer
│   │   ├── context_loader.rs      # Context retrieval
│   │   ├── pattern_engine.rs      # Pattern detection
│   │   └── types.rs               # Relationship types
│   ├── git/                       # Git integration
│   │   ├── client/                # Git operations
│   │   │   ├── operations.rs      # Core git ops
│   │   │   ├── project_ops.rs     # Project management
│   │   │   ├── tree_builder.rs    # Tree construction
│   │   │   ├── diff_parser.rs     # Diff parsing
│   │   │   ├── branch_manager.rs  # Branch operations
│   │   │   └── code_sync.rs       # Sync operations
│   │   ├── store.rs               # Git persistence
│   │   └── types.rs               # Git types
│   ├── prompt/
│   │   └── unified_builder.rs     # System prompt assembly
│   ├── persona/
│   │   ├── default.rs             # Default persona
│   │   └── mod.rs                 # Persona management
│   ├── project/                   # Project management
│   │   ├── store.rs               # Project persistence
│   │   └── types.rs               # Project types
│   ├── file_system/               # File operations
│   │   └── operations.rs          # FS operations
│   ├── tools/                     # Tool implementations
│   │   ├── file_ops.rs            # File tool handlers
│   │   ├── project_context.rs     # Project context tools
│   │   └── types.rs               # Tool types
│   └── tasks/                     # Background tasks
│       ├── mod.rs                 # Task manager
│       ├── config.rs              # Task configuration
│       ├── backfill.rs            # Data backfilling
│       ├── code_sync.rs           # Code synchronization
│       ├── embedding_cleanup.rs   # Embedding maintenance
│       └── metrics.rs             # Metrics collection
└── tests/                         # Integration tests (17 suites, 127+ tests)
    ├── artifact_flow_test.rs                     # Artifact CRUD & isolation
    ├── code_embedding_test.rs                    # Code parsing & semantic search
    ├── context_builder_prompt_assembly_test.rs   # Context gathering & prompts
    ├── deepseek_live_test.rs                     # DeepSeek live API integration
    ├── e2e_data_flow_test.rs                     # End-to-end data flows
    ├── embedding_cleanup_test.rs                 # Orphan embedding removal
    ├── git_operations_test.rs                    # Git clone/import/sync
    ├── message_pipeline_flow_test.rs             # Message analysis pipeline
    ├── operation_engine_test.rs                  # Operation orchestration
    ├── phase5_providers_test.rs                  # Provider implementations
    ├── phase6_integration_test.rs                # Provider integration
    ├── phase7_routing_test.rs                    # Message routing logic
    ├── relationship_facts_test.rs                # Relationship tracking
    ├── rolling_summary_test.rs                   # Summary generation
    ├── storage_embedding_flow_test.rs            # Storage & embedding flows
    ├── websocket_connection_test.rs              # WebSocket connection mgmt
    └── websocket_message_routing_test.rs         # WebSocket message routing
```

---

## Development Workflow

### Running the Server

```bash
# Development mode with hot reload (requires cargo-watch)
cargo watch -x run

# With debug logging
RUST_LOG=debug cargo run

# Specific module logging
RUST_LOG=mira_backend::operations=trace cargo run

# Production build
cargo build --release
./target/release/mira-backend
```

### Testing

```bash
# Run all tests
cargo test

# Run specific test file
cargo test --test operation_engine_test

# Run with output
cargo test -- --nocapture

# Integration tests (requires Qdrant running)
cargo test --test phase6_integration_test
```

### Database Migrations

```bash
# Create new migration
sqlx migrate add migration_name

# Run migrations
sqlx migrate run

# Revert last migration
sqlx migrate revert

# Check migration status
sqlx migrate info
```

### Code Quality

```bash
# Run clippy (linter)
cargo clippy

# Fix automatically
cargo clippy --fix

# Format code
cargo fmt

# Check formatting
cargo fmt -- --check
```

---

## Common Development Tasks

### Adding a New LLM Provider

1. Create provider in `src/llm/provider/my_provider.rs`
2. Implement `LlmProvider` trait
3. Add to `AppState` initialization
4. Update configuration in `.env`

```rust
// Example provider implementation
pub struct MyProvider {
    api_key: String,
    model: String,
}

#[async_trait]
impl LlmProvider for MyProvider {
    async fn create(
        &self,
        messages: Vec<Message>,
        system: String,
        tools: Option<Vec<Value>>,
    ) -> Result<String> {
        // Implementation
    }
}
```

### Adding a New Delegation Tool

1. Define tool schema in `src/operations/delegation_tools.rs`
2. Add to `get_delegation_tools()` function
3. Handle in `DelegationHandler::delegate_to_deepseek()`

```rust
pub fn my_tool_schema() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "my_tool",
            "description": "What this tool does",
            "parameters": {
                "type": "object",
                "properties": {
                    "param": {"type": "string", "description": "..."}
                },
                "required": ["param"]
            }
        }
    })
}
```

### Adding Custom Context to Prompts

1. Extend context types in `src/operations/types.rs`
2. Modify `ContextBuilder::build_system_prompt()`
3. Pass through handler chain

### Debugging Operations

```bash
# Enable trace logging for operations
RUST_LOG=mira_backend::operations=trace cargo run

# Check operation state in database
sqlite3 mira.db "SELECT * FROM operations WHERE id = 'op-id';"

# View operation events
sqlite3 mira.db "SELECT * FROM operation_events WHERE operation_id = 'op-id' ORDER BY sequence_number;"

# Check artifacts
sqlite3 mira.db "SELECT id, path, language FROM artifacts WHERE operation_id = 'op-id';"
```

---

## Configuration

### Environment Variables

```bash
# Server
MIRA_HOST=0.0.0.0
MIRA_PORT=3001
MIRA_ENV=development  # development | staging | production

# Database
DATABASE_URL=sqlite://mira.db
MIRA_SQLITE_MAX_CONNECTIONS=10

# Qdrant
QDRANT_URL=http://localhost:6333
QDRANT_API_KEY=optional

# OpenAI / GPT-5
OPENAI_API_KEY=sk-...
OPENAI_MODEL=gpt-5-0314
OPENAI_EMBEDDING_MODEL=text-embedding-3-large
GPT5_REASONING=medium      # Default reasoning effort: low/medium/high
GPT5_VERBOSITY=medium      # Reasoning verbosity: low/medium/high

# DeepSeek
DEEPSEEK_API_KEY=...
DEEPSEEK_MODEL=deepseek-reasoner

# Memory Configuration
SALIENCE_MIN_FOR_EMBED=0.6
EMBED_HEADS=semantic,code,documents,relationship,summary
MAX_RECALLED_MESSAGES=10
SUMMARY_GENERATION_INTERVAL=10
USE_ROLLING_SUMMARIES=true

# Context Gathering
CONTEXT_RECENT_MESSAGES=5
CONTEXT_SEMANTIC_MATCHES=10

# Git Integration
GIT_ENABLED=true
DEFAULT_REPO_PATH=/path/to/repos

# Logging
RUST_LOG=info,mira_backend=debug
```

### Database Setup

```sql
-- Enable performance optimizations
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;

-- Check database integrity
PRAGMA integrity_check;
```

### Qdrant Setup

```bash
# Run Qdrant with Docker
docker run -p 6333:6333 qdrant/qdrant:latest

# Or with docker-compose
docker-compose up qdrant

# Health check
curl http://localhost:6333/health
```

---

## Testing Strategy

### Unit Tests

Located in `src/` files alongside implementation:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_something() {
        // Test implementation
    }
}
```

### Integration Tests

Located in `tests/` directory:
- `operation_engine_test.rs` - Operation lifecycle
- `phase6_integration_test.rs` - Full orchestration
- `e2e_data_flow_test.rs` - Message → Storage → Retrieval

### Test Database Setup

```rust
async fn create_test_db() -> Arc<SqlitePool> {
    let pool = SqlitePoolOptions::new()
        .connect(":memory:")
        .await
        .expect("Failed to create in-memory database");
    
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("Failed to run migrations");
    
    Arc::new(pool)
}
```

### Mocking LLM Providers

Tests use fake API keys and expect failures for actual API calls:
```rust
let gpt5 = Gpt5Provider::new(
    "test-key".to_string(),
    "gpt-5-preview".to_string(),
    4000,
    "medium".to_string(),
    "medium".to_string(),
);
```

For integration tests that need real API calls, set environment variables.

---

## Troubleshooting

### Common Issues

**1. Tests failing with `OperationEngine::new()` signature mismatch**
- Engine constructor requires 7 parameters:
  ```rust
  OperationEngine::new(
      db: Arc<SqlitePool>,
      gpt5: Gpt5Provider,
      deepseek: DeepSeekProvider,
      memory_service: Arc<MemoryService>,
      relationship_service: Arc<RelationshipService>,
      git_client: GitClient,
      code_intelligence: Arc<CodeIntelligenceService>,
  )
  ```

**2. SQLite database locked**
- Enable WAL mode: `PRAGMA journal_mode=WAL`
- Increase connection pool size in config
- Check for long-running transactions

**3. Qdrant connection failures**
- Verify Qdrant is running: `curl http://localhost:6333/health`
- Check `QDRANT_URL` in `.env`
- Ensure collections are created on startup

**4. Embedding generation not working**
- Check `SALIENCE_MIN_FOR_EMBED` threshold
- Verify OpenAI API key is valid
- Check `EMBED_HEADS` configuration

**5. Operations failing silently**
- Enable trace logging: `RUST_LOG=mira_backend::operations=trace`
- Check operation events in database
- Verify error handling wrapper is emitting `Failed` events

**6. WebSocket disconnections**
- Implement ping/pong in client
- Check for network timeouts
- Verify cancellation token handling

### Debug Commands

```bash
# Check database integrity
sqlite3 mira.db "PRAGMA integrity_check;"

# View recent operations
sqlite3 mira.db "SELECT id, kind, status, created_at FROM operations ORDER BY created_at DESC LIMIT 10;"

# Check embedding distribution across heads
sqlite3 mira.db "SELECT embedding_head, COUNT(*) FROM message_embeddings GROUP BY embedding_head;"

# Qdrant health check
curl http://localhost:6333/health

# View all collections
curl http://localhost:6333/collections

# Test WebSocket connection
websocat ws://localhost:3001/ws
```

### Logging

```bash
# Structured logging (JSON)
LOG_FORMAT=json cargo run

# Pretty printing (development)
LOG_FORMAT=pretty cargo run

# Module-specific debug
RUST_LOG=mira_backend::memory=debug,mira_backend::operations=trace cargo run
```

---

## Performance Optimization

### Database

- Enable WAL mode for better concurrency
- Add indexes on frequently queried columns
- Vacuum database periodically: `sqlite3 mira.db "VACUUM;"`
- Monitor query performance with `EXPLAIN QUERY PLAN`

### Embeddings

- Batch embedding generation where possible
- Increase salience threshold to reduce embedding volume
- Use appropriate Qdrant distance metric (cosine)
- Monitor Qdrant memory usage

### Memory

- Reduce `MAX_RECALLED_MESSAGES` if memory-constrained
- Implement context window truncation for large conversations
- Monitor for memory leaks in long-running operations

### Concurrency

- Tune SQLite connection pool size
- Use async/await throughout (no blocking operations)
- Implement request timeouts
- Add rate limiting for expensive operations

---

## Deployment

### Docker

```dockerfile
FROM rust:1.75 as builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y \
    libsqlite3-0 ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/mira-backend /usr/local/bin/
CMD ["mira-backend"]
```

### Docker Compose

```yaml
version: '3.8'
services:
  mira:
    build: .
    ports:
      - "3001:3001"
    environment:
      - DATABASE_URL=sqlite:///data/mira.db
      - QDRANT_URL=http://qdrant:6333
    volumes:
      - ./data:/data
    depends_on:
      - qdrant
  
  qdrant:
    image: qdrant/qdrant:latest
    ports:
      - "6333:6333"
    volumes:
      - qdrant_data:/qdrant/storage

volumes:
  qdrant_data:
```

### Production Checklist

- [ ] Enable WAL mode for SQLite
- [ ] Set `MIRA_ENV=production`
- [ ] Configure proper logging (JSON format)
- [ ] Set up monitoring (metrics, alerts)
- [ ] Implement health check endpoint
- [ ] Configure backups (SQLite + Qdrant snapshots)
- [ ] Use secrets management (not .env files)
- [ ] Enable HTTPS for external APIs
- [ ] Set up rate limiting
- [ ] Configure connection pool sizes appropriately

---

## Architecture Deep Dives

For comprehensive technical documentation, see:

- **[MIRA_BACKEND_WHITEPAPER.md](./MIRA_BACKEND_WHITEPAPER.md)** - Complete architectural reference
- **[Mira_Frontend_Whitepaper_FINAL.md](./Mira_Frontend_Whitepaper_FINAL.md)** - Frontend WebSocket protocol
- **[MIRA_FIXES_IMPLEMENTATION_GUIDE.md](./MIRA_FIXES_IMPLEMENTATION_GUIDE.md)** - Historical issue tracking

### Key Design Decisions

**Why Rust?**
- Type safety prevents entire classes of bugs
- Near-C performance for streaming
- Fearless concurrency
- Excellent async ecosystem (tokio)

**Why Dual Storage (SQLite + Qdrant)?**
- SQLite: Fast structured queries, ACID guarantees, simple deployment
- Qdrant: Purpose-built for vector search, scales horizontally
- Best of both worlds: structured + semantic search

**Why GPT-5 + DeepSeek?**
- Specialization: Each model excels in its domain
- Cost efficiency: DeepSeek for bulk code generation
- Quality: GPT-5's reasoning + DeepSeek's implementation
- Dynamic reasoning: High effort for planning, low for simple queries, medium for normal execution

**Why WebSocket over HTTP?**
- Real-time: Streaming updates during long operations
- Efficiency: Single persistent connection
- Bidirectional: Server can push updates proactively
- Cancellation: User can interrupt operations

---

## Contributing

### Code Style

- Follow Rust naming conventions (snake_case for functions, PascalCase for types)
- Use `rustfmt` for formatting (run `cargo fmt` before committing)
- Write descriptive commit messages
- Add tests for new features
- Update documentation when changing APIs

### Pull Request Process

1. Create feature branch from `main`
2. Implement changes with tests
3. Run full test suite: `cargo test`
4. Check for warnings: `cargo clippy`
5. Format code: `cargo fmt`
6. Update relevant documentation
7. Submit PR with clear description

### Documentation Standards

- Inline comments for complex logic
- Rustdoc for public APIs
- Update whitepaper for architectural changes
- Add examples for new features

---

## License

Proprietary

---

## Support

For questions or issues:
1. Check troubleshooting section above
2. Review whitepaper for architectural details
3. Search closed issues in repo
4. Open new issue with reproduction steps

---

**Happy coding!** 🚀
