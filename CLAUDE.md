# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Mira is an AI-powered coding assistant with a **Rust backend** and **React + TypeScript frontend** in a monorepo structure. The backend uses GPT 5.1 with variable reasoning effort levels, hybrid memory systems (SQLite + Qdrant), real-time WebSocket streaming, and comprehensive code intelligence including semantic graph analysis, git intelligence, and tool synthesis.

## Repository Structure

```
mira/
├── backend/          # Rust backend (WebSocket server, LLM orchestration, memory)
└── frontend/         # React + TypeScript frontend (Vite, Zustand, Monaco)
```

## Development Commands

### Backend (Rust)

```bash
cd backend

# Build
cargo build

# Run server (WebSocket on port 3001)
cargo run

# Run with debug logging
RUST_LOG=debug cargo run

# Module-specific logging
RUST_LOG=mira_backend::operations=trace cargo run

# Run all tests
cargo test

# Run specific test file
cargo test --test operation_engine_test

# Run tests with output
cargo test -- --nocapture

# Linting & formatting
cargo clippy
cargo fmt

# Database migrations
sqlx migrate run
sqlx migrate revert
sqlx migrate add <migration_name>
```

### Frontend (React + TypeScript)

```bash
cd frontend

# Development server (proxies to backend on port 3001)
npm run dev

# Build
npm run build

# Type checking
npm run type-check

# Tests
npm run test              # Run once
npm run test:watch        # Watch mode
npm run test:ui          # UI mode
npm run test:coverage    # With coverage

# Preview production build
npm run preview
```

## Running the Application

### Backend

```bash
cd backend

# Development mode
cargo run

# Production mode (after making changes)
cargo build --release
./target/release/mira-backend

# Background mode with logging
nohup ./target/release/mira-backend > /tmp/mira_backend.log 2>&1 &

# Stop backend
pkill -f mira-backend

# Check if running
lsof -i :3001
```

### Frontend

```bash
cd frontend

# Development mode
npm run dev

# Production mode
npm run build
npm run preview
```

**Important Notes**:
- Backend runs on port 3001 (WebSocket server)
- Frontend dev server proxies to backend on port 3001
- When making backend code changes, rebuild the release binary and restart the process
- Use `pkill -f mira-backend` to stop running backend processes before starting a new one

## Architecture

### Backend Architecture

**GPT 5.1 Single-Model with Variable Reasoning:**
- **GPT 5.1** handles all operations with variable reasoning effort (minimum/medium/high)
- **Budget Management** tracks daily/monthly spending with user-configurable limits
- **LLM Response Cache** targets 80%+ hit rate for cost optimization (SHA-256 key hashing)
- **Operation Engine** (`src/operations/engine/`) orchestrates complex workflows with status tracking
- **Reasoning Effort Selection** adapts model complexity to task requirements

**Memory Systems** (`src/memory/`):
- **Hybrid storage**: SQLite (50+ tables) + Qdrant (3 collections: code, conversation, git)
- **Semantic Code Understanding**: Semantic graph, call graph, design pattern detection
- **Git Intelligence**: Commit tracking, co-change patterns, author expertise, historical fixes
- **Personal Memory**: User profile, memory facts, learned behavioral patterns
- **Recall Engine**: Combines recent messages + semantic search + rolling summaries + code intelligence
- **Context Gathering**: Assembles recent messages, semantic results, file trees, and code intelligence before each LLM call

**WebSocket Protocol** (`src/api/ws/`):
- Two coexisting protocols: legacy chat + operations
- Real-time streaming with cancellation support
- Event-driven artifact delivery
- Port **3001** (not 8080, despite what some old docs say)

**Key Backend Modules:**
- `src/operations/engine/` - Modular operation orchestration (lifecycle, artifacts, events, status tracking)
- `src/memory/` - Memory service coordinating SQLite + Qdrant stores
- `src/memory/features/code_intelligence/` - Semantic graph, call graph, pattern detection
- `src/llm/provider/` - GPT 5.1 provider with reasoning effort, OpenAI embeddings
- `src/budget/` - Budget tracking with daily/monthly limits
- `src/cache/` - LLM response cache (SHA-256 hashing, 80%+ hit rate)
- `src/git/intelligence/` - Commit tracking, co-change analysis, expertise scoring
- `src/synthesis/` - Tool pattern detection and auto-generation
- `src/build/` - Build system integration, error tracking, fix learning
- `src/patterns/` - Reasoning pattern storage and replay
- `src/relationship/` - User context and fact storage
- `src/api/ws/chat/` - Chat routing and connection management

**Operation Lifecycle:**
```
PENDING → STARTED → DELEGATING → GENERATING → COMPLETED
                                               ↓
                                          FAILED
```

### Frontend Architecture

**State Management (Zustand):**
- `useChatStore` - Messages, streaming, artifacts
- `useWebSocketStore` - WebSocket connection management
- `useAppState` - Projects, sessions, UI state
- `useAuthStore` - Authentication state
- `useUIStore` - UI-specific state

**Key Frontend Concepts:**
- **WebSocket communication**: Real-time streaming from backend via `/ws` endpoint
- **Artifacts**: Code blocks from LLM that can be saved/applied to files
- **Monaco Editor**: Embedded code editor for artifact viewing/editing
- **Project context**: Attaches git repository context to conversations
- **File browser**: Navigate and select files from active project

**Component Structure:**
- `src/components/` - React components (ChatArea, ArtifactPanel, FileBrowser, etc.)
- `src/stores/` - Zustand state stores
- `src/services/` - Backend command service
- `src/hooks/` - Custom React hooks

## Prerequisites

- **Rust 1.91** (backend - target version for this dev machine)
- **Rust Edition 2024** (use latest stable Rust edition)
- **Stable Crates Only** (always use current stable versions of all dependencies)
- **Node.js 18+** (frontend)
- **SQLite 3.35+** (backend database)
- **Qdrant** running on `localhost:6333` (vector database)
- **API Keys**: OpenAI (GPT 5.1 + embeddings)

## Environment Setup

### Backend

Create `backend/.env` from `backend/.env.example`:

```bash
# Server
MIRA_PORT=3001
MIRA_ENV=development

# Database
DATABASE_URL=sqlite://mira.db

# Qdrant
QDRANT_URL=http://localhost:6333

# OpenAI (GPT 5.1 + Embeddings)
OPENAI_API_KEY=sk-...
GPT5_MODEL=gpt-5.1
GPT5_REASONING_DEFAULT=medium
OPENAI_EMBEDDING_MODEL=text-embedding-3-large

# Budget Management
BUDGET_DAILY_LIMIT_USD=5.0
BUDGET_MONTHLY_LIMIT_USD=150.0

# Cache Configuration
CACHE_ENABLED=true
CACHE_TTL_SECONDS=86400
```

### Frontend

The frontend proxies to backend port 3001 (configured in `vite.config.js`).

## Testing Strategy

### Backend Tests

- **Integration tests** in `backend/tests/` (17 suites, 127+ tests)
- Tests use in-memory SQLite and fake API keys
- Run `cargo test` before committing
- Key test files:
  - `operation_engine_test.rs` - Operation orchestration
  - `git_operations_test.rs` - Git integration
  - `message_pipeline_flow_test.rs` - Message analysis
  - `e2e_data_flow_test.rs` - End-to-end flows

### Frontend Tests

- **Vitest** with React Testing Library
- Located in `src/__tests__/`
- Run `npm run test` before committing

## Working with Operations

Operations are complex multi-step workflows tracked through state transitions. When adding/modifying operation logic:

1. Define operation kind in `src/operations/types.rs` (`operation_kinds`)
2. Update operation engine in `src/operations/engine/orchestration.rs`
3. Add tool schemas in `src/operations/delegation_tools.rs`
4. Update DeepSeekOrchestrator in `src/operations/engine/deepseek_orchestrator.rs`
5. Emit events via channels for real-time frontend updates

**Critical**: The `OperationEngine::new()` constructor requires 7 parameters:
```rust
OperationEngine::new(
    db: Arc<SqlitePool>,
    deepseek: DeepSeekProvider,
    memory_service: Arc<MemoryService>,
    relationship_service: Arc<RelationshipService>,
    git_client: GitClient,
    code_intelligence: Arc<CodeIntelligenceService>,
    sudo_service: Option<Arc<SudoPermissionService>>,
)
```

## Common Pitfalls

1. **Backend port confusion**: The backend runs on port **3001**, not 8080 (some old docs/comments say 8080)
2. **Qdrant dependency**: Many features require Qdrant running on `localhost:6333`
3. **SQLite WAL mode**: Enable with `PRAGMA journal_mode=WAL` for better concurrency
4. **Test isolation**: Backend tests use in-memory databases; don't rely on persistent state
5. **WebSocket protocols**: Two coexisting protocols (legacy chat + operations) - don't confuse them
6. **Cargo edition**: Set to "2024" in Cargo.toml (non-standard, verify compatibility)

## Git Integration

The system supports git operations for project context:
- Clone/import repositories via `src/git/client/operations.rs`
- Project management in `src/git/client/project_ops.rs`
- File tree building in `src/git/client/tree_builder.rs`
- Diff parsing in `src/git/client/diff_parser.rs`

Repositories stored in `backend/repos/` (or `backend/test_repos/` for tests).

## Memory & Embeddings

**When debugging memory issues:**
- Check `SALIENCE_MIN_FOR_EMBED` threshold (default 0.6)
- Verify OpenAI API key for embeddings
- Inspect `EMBED_HEADS` configuration
- Run `backend/scripts/reset_embeddings.sh` if embeddings are corrupted

**Storage locations:**
- SQLite: `backend/mira.db` (messages, operations, artifacts)
- Qdrant: Vector embeddings across 5 collections
- Git repos: `backend/repos/`
- Documents: `backend/storage/documents/`

## Debugging

**Backend logging:**
```bash
# Full debug
RUST_LOG=debug cargo run

# Operations trace
RUST_LOG=mira_backend::operations=trace cargo run

# Check database
sqlite3 backend/mira.db "SELECT * FROM operations ORDER BY created_at DESC LIMIT 10;"

# Check Qdrant
curl http://localhost:6333/health
curl http://localhost:6333/collections
```

**Frontend debugging:**
- Check browser console for WebSocket errors
- Use React DevTools for state inspection
- Zustand stores are logged to console in dev mode

## Code Style

### General Rules (All Code)

**File Headers:**
- Every code file must start with a comment containing the file path/filename
- Example (Rust): `// backend/src/operations/engine/mod.rs`
- Example (TypeScript): `// frontend/src/hooks/useMessageHandler.ts`

**Comments:**
- No emojis anywhere - not in documentation, code, or comments
- No phased comments (e.g., "Phase 1:", "Phase 2:", "Step 1:", etc.)
- Code comments should only explain what that section of code does
- Keep comments concise and focused on the "what" and "why", not the "how"

**Backend (Rust):**
- Run `cargo fmt` before committing
- Run `cargo clippy` and address warnings
- Follow Rust naming conventions (snake_case for functions, PascalCase for types)
- Add inline comments for complex logic
- File header format: `// backend/src/path/to/file.rs`

**Frontend (TypeScript):**
- ESLint configured with React rules
- Use TypeScript strict mode
- Functional components with hooks (no class components)
- File header format: `// frontend/src/path/to/file.ts` or `.tsx`

## Important Files

- `backend/src/state.rs` - AppState (DI container for all services)
- `backend/src/operations/engine/orchestration.rs` - Main operation execution loop
- `backend/src/memory/service/core_service.rs` - Memory service coordination
- `backend/src/api/ws/chat/unified_handler.rs` - Chat routing logic
- `frontend/src/App.tsx` - Main application component
- `frontend/src/stores/useChatStore.ts` - Chat state management
- `frontend/src/services/BackendCommands.ts` - Backend API client

## External Dependencies

- **Qdrant** vector database for embeddings (must run separately)
- **OpenAI API** for text-embedding-3-large embeddings only
- **DeepSeek API** for dual-model LLM (deepseek-chat + deepseek-reasoner)
- **SQLite** for structured storage (embedded)

## Additional Documentation

- `backend/README.md` - Comprehensive backend documentation
- `backend/WHITEPAPER.md` - Detailed architectural reference
- `backend/.env.example` - Environment variable template
- `PROGRESS.md` - **Session progress log (UPDATE AT END OF EACH SESSION)** - Tracks detailed technical progress by milestone/phase including goals, outcomes, files changed, git commits, and technical decisions
- `HOUSECLEANING_SUMMARY.md` - Complete report of November 2025 refactoring effort
- `ISSUES_TO_CREATE.md` - Catalogued technical debt items for future work
- `frontend/docs/STATE_BOUNDARIES.md` - Frontend state management architecture
