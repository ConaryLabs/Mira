# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Mira is an AI-powered coding assistant with a **Rust backend** and **React + TypeScript frontend** in a monorepo structure. The backend uses OpenAI GPT-5.1 family with multi-tier routing, hybrid memory systems (SQLite + Qdrant), real-time WebSocket streaming, and comprehensive code intelligence including semantic graph analysis, git intelligence, and tool synthesis.

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

### Service Management (Recommended)

Both backend and frontend run as systemd user services. Use `mira-ctl` to manage them:

```bash
# Start/stop/restart services
mira-ctl start all           # Start both backend and frontend
mira-ctl stop all            # Stop both services
mira-ctl restart backend     # Restart just the backend
mira-ctl restart frontend    # Restart just the frontend

# Check status
mira-ctl status              # Show status of both services

# View logs
mira-ctl logs backend        # Show backend logs
mira-ctl logs backend -f     # Follow backend logs (live)
mira-ctl logs frontend -f    # Follow frontend logs

# After code changes
mira-ctl rebuild             # Build release binary and restart backend
mira-ctl restart frontend    # Restart frontend (usually auto-reloads)
```

**Direct systemctl commands also work:**
```bash
systemctl --user restart mira-backend
systemctl --user restart mira-frontend
systemctl --user status mira-backend
journalctl --user -u mira-backend -f
```

### Manual Running (Alternative)

```bash
# Backend (development mode)
cd backend
cargo run

# Backend (production mode)
cargo build --release
./target/release/mira-backend

# Frontend
cd frontend
npm run dev
```

**Service Details:**
- Backend: Port 3001 (WebSocket server), auto-restarts on failure
- Frontend: Port 5173 (Vite dev server), auto-restarts on failure
- Services are enabled by default (auto-start on login)
- Service files: `~/.config/systemd/user/mira-{backend,frontend}.service`

## Architecture

### Backend Architecture

**OpenAI GPT-5.1 Multi-Model Routing:**
- **4-tier model routing** intelligently routes tasks to optimal model tier
- **Budget Management** tracks daily/monthly spending with user-configurable limits
- **LLM Response Cache** targets 80%+ hit rate for cost optimization (SHA-256 key hashing)
- **Operation Engine** (`src/operations/engine/`) orchestrates complex workflows with status tracking
- **OpenAI Responses API** uses the new `/v1/responses` endpoint (not legacy Chat Completions)

**Multi-Model Routing (OpenAI GPT-5.1 Family):**

The model router (`src/llm/router/`) intelligently routes tasks to the optimal model tier:

| Tier | Model | Use Case | Reasoning | Pricing |
|------|-------|----------|-----------|---------|
| Fast | GPT-5.1 Mini | File ops, search, simple queries | - | $0.25/$2 per 1M |
| Voice | GPT-5.1 | User chat, explanations, personality | medium | $1.25/$10 per 1M |
| Code | GPT-5.1-Codex-Max | Code generation, refactoring, tests | high | $1.25/$10 per 1M |
| Agentic | GPT-5.1-Codex-Max | Long-running autonomous tasks (24h+) | xhigh | $1.25/$10 per 1M |

Routing rules:
- **Fast tier**: File listing, grep, search, simple metadata
- **Voice tier**: User-facing chat, explanations (default)
- **Code tier**: Architecture, refactoring, code review, >50k tokens, >3 files
- **Agentic tier**: Full implementations, migrations, large refactors

**Memory Systems** (`src/memory/`):
- **Hybrid storage**: SQLite (50+ tables) + Qdrant (3 collections: code, conversation, git)
- **Semantic Code Understanding**: Semantic graph, call graph, design pattern detection
- **Git Intelligence**: Commit tracking, co-change patterns, author expertise, historical fixes
- **Personal Memory**: User profile, memory facts, learned behavioral patterns
- **Recall Engine**: Combines recent messages + semantic search + rolling summaries + code intelligence
- **Context Gathering**: Assembles recent messages, semantic results, file trees, and code intelligence before each LLM call

**Context Architecture** (conversation memory):

| Layer | Purpose | Config | Approx Tokens |
|-------|---------|--------|---------------|
| **LLM Message Array** | Direct conversation turns | `MIRA_LLM_MESSAGE_HISTORY_LIMIT=12` | ~3-5K |
| **Rolling Summary** | Compressed older history (every 100 msgs) | `MIRA_SUMMARY_ROLLING_ENABLED=true` | ~2.5K |
| **Semantic Search** | Relevant memories from any time | `MIRA_CONTEXT_SEMANTIC_MATCHES=10` | ~1-2K |

Key insight: Recent messages go in the message array (what LLM responds to), rolling summaries handle compression for older context, semantic search surfaces relevant distant memories.

**WebSocket Protocol** (`src/api/ws/`):
- Two coexisting protocols: legacy chat + operations
- Real-time streaming with cancellation support
- Event-driven artifact delivery
- Port **3001** (not 8080, despite what some old docs say)

**Key Backend Modules:**
- `src/operations/engine/` - Modular operation orchestration (lifecycle, artifacts, events, status tracking)
- `src/memory/` - Memory service coordinating SQLite + Qdrant stores
- `src/memory/features/code_intelligence/` - Semantic graph, call graph, pattern detection
- `src/llm/provider/` - OpenAI GPT-5.1 providers (Responses API) and embeddings (text-embedding-3-large)
- `src/llm/router/` - Multi-model routing: Fast/Voice/Code/Agentic tiers with task classification
- `src/budget/` - Budget tracking with daily/monthly limits
- `src/cache/` - LLM response cache (SHA-256 hashing, 80%+ hit rate)
- `src/git/intelligence/` - Commit tracking, co-change analysis, expertise scoring
- `src/synthesis/` - Tool pattern detection and auto-generation
- `src/build/` - Build system integration, error tracking, fix learning
- `src/patterns/` - Reasoning pattern storage and replay
- `src/relationship/` - User context and fact storage
- `src/system/` - System environment detection (OS, package manager, shell, tools)
- `src/prompt/` - Prompt building with system context, memory, and code intelligence
- `src/api/ws/chat/` - Chat routing and connection management

**System Context** (`src/system/`):
- Detects OS (Linux distro from /etc/os-release, macOS via sw_vers, Windows)
- Identifies primary package manager (apt, brew, dnf, pacman, etc.)
- Detects shell (bash, zsh, fish, etc.)
- Scans for available CLI tools (git, docker, node, cargo, python, etc.)
- Cached at startup via lazy_static for zero-cost runtime access
- Injected into prompts so LLM uses platform-appropriate commands

**Time Awareness**:
- Current date/time injected into system context at prompt build time
- Uses `chrono::Local::now()` with system timezone
- Format: "Thursday, December 05, 2025 at 08:22 PM (PST)"
- LLM knows current date without user mentioning it

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
- **Latest Stable Crates** - Always use the current latest stable version of all dependencies. When adding new dependencies or upgrading existing ones, check crates.io for the latest version. No pinning to old versions unless there's a specific compatibility issue.
- **Node.js 18+** (frontend)
- **SQLite 3.35+** (backend database)
- **Qdrant 1.16+** running on `localhost:6334` (gRPC) and `localhost:6333` (HTTP)
- **API Keys**: OpenAI (GPT-5.1 family + text-embedding-3-large)

### Starting Qdrant

```bash
cd backend
# Start Qdrant with config file (creates data/qdrant/ directory)
./bin/qdrant --config-path ./config/config.yaml

# Or run in background
nohup ./bin/qdrant --config-path ./config/config.yaml > /tmp/qdrant.log 2>&1 &

# Verify it's running
curl http://localhost:6333  # Should return version info
```

## Environment Setup

### Backend

Create `backend/.env` from `backend/.env.example`:

```bash
# Server
MIRA_PORT=3001
MIRA_ENV=development

# Database
DATABASE_URL=sqlite://data/mira.db

# Qdrant (gRPC port)
QDRANT_URL=http://localhost:6334

# OpenAI (GPT-5.1 family + embeddings)
OPENAI_API_KEY=your-openai-api-key

# Model Router Configuration (4-tier routing)
MODEL_ROUTER_ENABLED=true
MODEL_FAST=gpt-5.1-mini           # Fast tier: file ops, search
MODEL_VOICE=gpt-5.1               # Voice tier: user chat
MODEL_CODE=gpt-5.1-codex-max      # Code tier: code generation
MODEL_AGENTIC=gpt-5.1-codex-max   # Agentic tier: long-running tasks
ROUTE_CODE_TOKEN_THRESHOLD=50000  # Tokens before upgrading to Code tier
ROUTE_CODE_FILE_COUNT=3           # Files before upgrading to Code tier

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

### Scenario Tests (mira-test CLI)

End-to-end scenario testing via the `mira-test` binary:

```bash
cd backend

# Run all scenarios
cargo run --bin mira-test -- run ./scenarios/

# Run specific scenario
cargo run --bin mira-test -- run ./scenarios/smoke_test.yaml

# List available scenarios
cargo run --bin mira-test -- list ./scenarios/

# Validate YAML syntax
cargo run --bin mira-test -- validate ./scenarios/

# Filter by tags
cargo run --bin mira-test -- run ./scenarios/ --tags smoke
```

**Scenario Format** (`scenarios/*.yaml`):
```yaml
name: "Test Name"
description: "What this tests"
tags: ["smoke", "tools"]
timeout_seconds: 120

setup:
  create_files:
    - path: "test.txt"
      content: "Hello"

steps:
  - name: "Step name"
    prompt: "User message to send"
    timeout_seconds: 60
    assertions:
      - type: completed_successfully
      - type: tool_executed
        tool_name: list_project_files
        success: true
      - type: response_contains
        text: "expected text"

cleanup:
  remove_project: true
```

**Session Isolation:** Each test run creates an isolated session with its own conversation history. The `session_id` is passed in Chat messages to route to the correct context.

**Available Assertions:**
- `completed_successfully` - Operation finished without error
- `tool_executed` - Specific tool was called with expected success
- `response_contains` - Response includes expected text
- `file_exists` / `file_contains` - File system checks
- `event_received` - Specific event type was emitted

### Backend Unit/Integration Tests

- **Integration tests** in `backend/tests/` (17 suites, 160+ tests)
- Tests use in-memory SQLite
- Run `cargo test` before committing
- Key test files:
  - `operation_engine_test.rs` - Operation orchestration
  - `git_operations_test.rs` - Git integration
  - `message_pipeline_flow_test.rs` - Message analysis (requires real LLM)
  - `e2e_data_flow_test.rs` - End-to-end flows (requires real LLM)
  - `rolling_summary_test.rs` - Summarization (requires real LLM)
  - `context_oracle_e2e_test.rs` - Context oracle (requires real LLM)

**Environment for Tests:**
- Tests load `backend/.env` via `dotenv::dotenv()` for API keys
- **`OPENAI_API_KEY` is required** - tests fail without it (no graceful skip)
- Qdrant tests require Qdrant running on `localhost:6334` (gRPC)
- LLM integration tests make real API calls to OpenAI

### Frontend Tests

- **Vitest** with React Testing Library
- Located in `src/__tests__/`
- Run `npm run test` before committing

## Working with Operations

Operations are complex multi-step workflows tracked through state transitions. When adding/modifying operation logic:

1. Define operation kind in `src/operations/types.rs` (`operation_kinds`)
2. Update operation engine in `src/operations/engine/orchestration.rs`
3. Add tool schemas in `src/operations/delegation_tools.rs` (use `get_delegation_tools()`)
4. Update context building in `src/operations/engine/context.rs`
5. Emit events via channels for real-time frontend updates

**Tool Schema Format** (OpenAI Function Calling):
```json
{
  "type": "function",
  "name": "tool_name",
  "description": "What the tool does",
  "parameters": { ... }
}
```

### CLI Architecture

The CLI (`backend/src/cli/`) provides a Claude Code-style command line interface that connects to the same backend via WebSocket.

**Key CLI Modules:**
- `repl.rs` - Interactive REPL loop with session management
- `ws_client.rs` - WebSocket client with event parsing
- `display/` - Terminal output handling (streaming, colors, spinners)
- `session.rs` - Session management and picker
- `commands/` - Custom slash command loading
- `project.rs` - Project detection and context building

**CLI Features:**
- Interactive REPL with streaming responses
- Session management (create, resume, fork, list)
- Project context detection (auto-detects git repos)
- Custom slash commands (`.mira/commands/`)
- Multiple output formats: text (default), json, stream-json
- Sudo approval prompts (interactive Y/n)

## Feature Parity Requirements

**CRITICAL: Frontend (Web) and CLI must maintain feature parity.**

When implementing new features:
1. If the feature is user-facing, implement it in BOTH the frontend and CLI
2. Backend WebSocket messages should work identically for both clients
3. Interactive features (like sudo approval) need appropriate UX for each:
   - Frontend: Inline UI components with buttons
   - CLI: Terminal prompts with keyboard input
4. Test both interfaces when adding new functionality

**Current feature parity examples:**
- Streaming responses: Both support real-time token streaming
- Session management: Both can create, resume, fork sessions
- Sudo approval: Frontend shows inline buttons, CLI shows Y/n prompt
- Tool execution: Both display tool summaries and results

## Common Pitfalls

1. **Backend port confusion**: The backend runs on port **3001**, not 8080 (some old docs/comments say 8080)
2. **Qdrant dependency**: Many features require Qdrant running on `localhost:6334` (gRPC)
3. **SQLite WAL mode**: Enable with `PRAGMA journal_mode=WAL` for better concurrency
4. **Test isolation**: Backend tests use in-memory databases; don't rely on persistent state
5. **WebSocket protocols**: Two coexisting protocols (legacy chat + operations) - don't confuse them
6. **Cargo edition**: Set to "2024" in Cargo.toml (non-standard, verify compatibility)
7. **Feature parity**: New features must work in BOTH frontend and CLI - don't forget either interface

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
- Verify OpenAI API key for embeddings (uses text-embedding-3-large)
- Inspect `EMBED_HEADS` configuration
- Run `backend/scripts/db-reset-qdrant.sh` if embeddings are corrupted

**Storage locations:**
- SQLite: `backend/data/mira.db` (messages, operations, artifacts)
- Qdrant: Vector embeddings across 3 collections (code, conversation, git)
- Git repos: `backend/repos/`
- Documents: `backend/storage/documents/`

## Database Management

### Reset Scripts

All scripts are in `backend/scripts/` and should be run from the backend directory.

| Script | Purpose |
|--------|---------|
| `db-reset.sh` | Full reset - wipes SQLite + Qdrant |
| `db-reset-sqlite.sh` | Reset SQLite only (preserves embeddings) |
| `db-reset-qdrant.sh` | Reset Qdrant only (preserves structured data) |
| `db-reset-test.sh` | Clean up test Qdrant collections |

### Common Operations

**Full database reset (nuclear option):**
```bash
cd backend
./scripts/db-reset.sh
```

**Reset just SQLite (keep embeddings):**
```bash
cd backend
./scripts/db-reset-sqlite.sh
```

**Reset just embeddings (keep messages/operations):**
```bash
cd backend
./scripts/db-reset-qdrant.sh
```

**Clean up after tests:**
```bash
cd backend
./scripts/db-reset-test.sh
```

**Manual SQLite operations:**
```bash
# View tables
sqlite3 data/mira.db ".tables"

# Run specific query
sqlite3 data/mira.db "SELECT COUNT(*) FROM memory_entries;"

# Run migrations manually
DATABASE_URL="sqlite:./data/mira.db" sqlx migrate run
```

**Manual Qdrant operations:**
```bash
# List collections
curl http://localhost:6333/collections

# Delete specific collection
curl -X DELETE http://localhost:6333/collections/mira_code

# Check collection info
curl http://localhost:6333/collections/mira_conversation
```

## Debugging

**Backend logging:**
```bash
# Full debug
RUST_LOG=debug cargo run

# Operations trace
RUST_LOG=mira_backend::operations=trace cargo run

# Check database
sqlite3 backend/data/mira.db "SELECT * FROM operations ORDER BY created_at DESC LIMIT 10;"

# Check Qdrant (HTTP API)
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
- No emojis anywhere - not in documentation, code, comments, or git commits
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
- **OpenAI API** for GPT-5.1 family (LLM) and text-embedding-3-large (embeddings)
- **SQLite** for structured storage (embedded)

## Additional Documentation

- `backend/README.md` - Comprehensive backend documentation
- `backend/WHITEPAPER.md` - Detailed architectural reference
- `backend/.env.example` - Environment variable template
- `SESSION.md` - **Session log (UPDATE AT END OF EACH SESSION)** - Add commit hash and brief summary
- `HOUSECLEANING_SUMMARY.md` - Complete report of November 2025 refactoring effort
- `ISSUES_TO_CREATE.md` - Catalogued technical debt items for future work
- `frontend/docs/STATE_BOUNDARIES.md` - Frontend state management architecture
