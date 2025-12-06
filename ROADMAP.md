# Mira Roadmap: Unified Programming + Personal Context Oracle

## Vision

Mira is a next-generation AI coding assistant combining:
- **Programming Context Oracle** from mira-cli (semantic code understanding, git intelligence, pattern detection)
- **Personal Memory System** from Mira (user preferences, communication style, learned patterns)
- **Web-based Interface** for accessibility and collaboration
- **Gemini 3 Pro** for state-of-the-art reasoning with variable thinking levels

The result is a well-rounded assistant that understands both your code and you as a developer.

## Core Architecture

### LLM Stack
- **Primary Model**: Gemini 3 Pro with variable thinking levels (low/high)
- **Embeddings**: Gemini gemini-embedding-001 (3072 dimensions)
- **Cost Optimization**: 80%+ cache hit rate target, budget tracking with daily/monthly limits

### Storage Architecture
- **SQLite**: Structured data (50+ tables across 9 migrations)
- **Qdrant**: 3 collections for vector embeddings
  - `code`: Programming context (symbols, semantic nodes, patterns)
  - `conversation`: Messages, personal context, documents
  - `git`: Commits, co-change patterns, historical fixes

### Backend
- **Language**: Rust
- **Web Server**: WebSocket-based (port 3001)
- **Key Systems**:
  - Code Intelligence (AST, semantic graph, call graph)
  - Git Intelligence (commits, co-change, expertise)
  - Memory System (conversation, facts, patterns)
  - Operations Engine (workflow orchestration)
  - Tool Synthesis (auto-generate custom tools)
  - Build System Integration (error tracking, fix learning)
  - Budget Management (cost tracking, caching)

### Frontend
- **Framework**: React + TypeScript
- **State Management**: Zustand
- **Build Tool**: Vite
- **Key Features**:
  - Real-time WebSocket streaming
  - Monaco code editor
  - File browser for projects
  - Artifact management
  - Activity panel

## Core Capabilities

### 1. Semantic Code Understanding

**What**: Every code symbol gets analyzed for purpose, concepts, and domain labels.

**How**:
- AST parsing creates `code_elements` with hierarchical relationships
- LLM analyzes each symbol to create `semantic_nodes` with purpose/concepts
- `semantic_edges` link related code (semantic similarity, shared domain, co-change)
- `concept_index` enables concept-based search ("find all authentication code")

**Value**:
- Search by concept, not just keywords
- Understand code relationships beyond syntax
- Context-aware code generation

**Example**:
```
User: "Find all database access code"
Mira: Searches concept_index for "database" concept
      Returns functions tagged with ["database", "persistence", "storage"]
```

### 2. Git Intelligence

**What**: Deep understanding of git history for co-change prediction and expertise tracking.

**How**:
- `git_commits` stores full commit history with file changes
- `file_cochange_patterns` tracks files frequently modified together
- `blame_annotations` links code lines to authors and commits
- `author_expertise` scores developer expertise by file/domain
- `historical_fixes` links error patterns to past fix commits

**Value**:
- "Files often changed with this one" suggestions
- Find experts for specific code areas
- Learn from past fixes for similar errors

**Example**:
```
User: Edits src/auth/validator.rs
Mira: "This file is often changed with src/auth/middleware.rs and src/api/routes.rs"
      "John Doe has 87% expertise in authentication code"
```

### 3. Call Graph Analysis

**What**: Explicit caller-callee relationships for impact analysis.

**How**:
- `call_graph` table stores function call relationships
- Graph traversal finds all callers/callees
- Impact analysis shows what breaks if a function changes

**Value**:
- Understand function dependencies
- Predict impact of changes
- Context-aware refactoring

**Example**:
```
User: "What calls validate_user()?"
Mira: Traverses call_graph
      Returns: authenticate_request(), check_permissions(), login_handler()
```

### 4. Design Pattern Detection

**What**: Automatically detect design patterns (Factory, Repository, Strategy, etc.).

**How**:
- LLM analyzes code structure and relationships
- `design_patterns` table stores detected patterns with confidence scores
- `pattern_validation_cache` caches LLM analysis results
- Patterns guide code generation

**Value**:
- Generate code matching existing patterns
- Architectural insight
- Pattern-aware refactoring

**Example**:
```
User: "Create a new data access class"
Mira: Detects Repository pattern in existing code
      Generates new repository following the same pattern
```

### 5. Build System Integration

**What**: Persistent error tracking with automatic context injection.

**How**:
- `build_runs` tracks all build/test executions
- `build_errors` stores errors with hash-based deduplication
- `error_resolutions` links errors to fix commits
- Recent build errors auto-injected into operation context

**Value**:
- Learn from past fixes
- Detect recurring errors
- Automatic error context

**Example**:
```
Build fails with "borrow checker error in src/main.rs:42"
Mira: Checks historical_fixes for similar borrow errors
      "This error was fixed in commit abc123 by adding explicit lifetimes"
```

### 6. Tool Synthesis

**What**: Automatically generate custom tools based on codebase patterns.

**How**:
- `tool_patterns` detects repetitive codebase patterns
- LLM generates Rust tools to automate patterns
- `synthesized_tools` compiles and tracks tools
- `tool_executions` measures effectiveness
- `tool_evolution_history` tracks tool improvements

**Value**:
- Codebase-specific automation
- Eliminate repetitive tasks
- Tools improve over time

**Example**:
```
Mira detects pattern: "Frequently adding new API endpoints with similar structure"
Mira synthesizes tool: "add_api_endpoint(name, method, auth_required)"
Tool generates route + handler + tests following project patterns
```

### 7. Personal Memory

**What**: Remember user preferences, coding style, and past decisions.

**How**:
- `user_profile` stores coding preferences, tech stack, communication style
- `memory_facts` stores key-value facts with confidence tracking
- `learned_patterns` captures behavioral patterns
- `rolling_summaries` compress conversation history

**Value**:
- Personalized assistance
- Consistency with past decisions
- Adaptive communication

**Example**:
```
User: "Add error handling"
Mira: Checks user_profile preference: "Prefers Result<T> over panicking"
      Checks memory_facts: "User's team uses anyhow for error handling"
      Generates code with anyhow::Result<T>
```

### 8. Reasoning Pattern Learning

**What**: Store successful coding patterns and replay for similar problems.

**How**:
- `reasoning_patterns` stores patterns with trigger types and reasoning chains
- `reasoning_steps` captures step-by-step thinking
- `pattern_usage` tracks success/failure
- Patterns evolve based on success rates

**Value**:
- Learn from past successes
- Faster problem-solving
- Accumulating expertise

**Example**:
```
Pattern: "Adding new database migration"
Trigger: User mentions "migration" + "database schema"
Reasoning chain:
  1. Check existing migrations for naming convention
  2. Create new migration file with next number
  3. Write up/down migrations
  4. Update schema documentation
  5. Add rollback test
Success rate: 92% (used 25 times)
```

### 9. Budget Management

**What**: Track API costs with 80%+ cache hit rate to minimize spending.

**How**:
- `budget_tracking` logs every LLM call with cost
- `budget_summary` aggregates daily/monthly spending
- `llm_cache` caches responses with SHA-256 key hashing
- Pre-call budget checks prevent overruns

**Value**:
- Predictable costs
- Massive savings via caching
- User-visible spending limits

**Example**:
```
Daily limit: $5.00
Current spend: $4.23
Cache hit rate: 84%
Estimated monthly: $132 (vs $825 without cache)
```

## Implementation Milestones

### Milestone 1: Foundation - COMPLETE

**Goal**: Core schema, Gemini 3 Pro integration, budget/cache system

**Deliverables**:
- [x] 9 SQL migrations (50+ tables)
- [x] 3 Qdrant collections setup (code, conversation, git)
- [x] Gemini 3 Pro provider implementation
- [x] Budget tracking module
- [x] LLM cache module
- [x] Basic tests (127+ passing)

**Key Files**:
- `backend/migrations/` - 9 migration files
- `backend/src/llm/provider/gemini3.rs` - Gemini 3 Pro provider
- `backend/src/budget/mod.rs` - Budget tracking
- `backend/src/cache/mod.rs` - LLM response cache
- `backend/src/memory/storage/qdrant/multi_store.rs` - Qdrant multi-collection store

### Milestone 2: Code Intelligence - COMPLETE

**Goal**: Semantic graph, call graph, pattern detection

**Deliverables**:
- [x] Semantic node/edge management (800+ lines)
- [x] Call graph traversal with impact analysis (600+ lines)
- [x] Design pattern detection - LLM-based (500+ lines)
- [x] Domain clustering with cohesion scoring (500+ lines)
- [x] Semantic analysis cache with LFU eviction (500+ lines)

**Key Files**:
- `backend/src/memory/features/code_intelligence/semantic.rs`
- `backend/src/memory/features/code_intelligence/call_graph.rs`
- `backend/src/memory/features/code_intelligence/patterns.rs`
- `backend/src/memory/features/code_intelligence/clustering.rs`
- `backend/src/memory/features/code_intelligence/cache.rs`

### Milestone 3: Git Intelligence - COMPLETE

**Goal**: Commit tracking, co-change analysis, expertise scoring

**Deliverables**:
- [x] Git commit indexing with file changes (520+ lines)
- [x] Co-change pattern detection with Jaccard confidence (440+ lines)
- [x] Blame annotation management with cache invalidation (510+ lines)
- [x] Author expertise scoring (40% commits + 30% lines + 30% recency) (470+ lines)
- [x] Historical fix matching with error normalization (750+ lines)

**Key Files**:
- `backend/src/git/intelligence/commits.rs`
- `backend/src/git/intelligence/cochange.rs`
- `backend/src/git/intelligence/blame.rs`
- `backend/src/git/intelligence/expertise.rs`
- `backend/src/git/intelligence/fixes.rs`

### Milestone 4: Tool Synthesis - COMPLETE

**Goal**: Auto-generate custom tools from codebase patterns

**Deliverables**:
- [x] Tool pattern detection (LLM-based)
- [x] Tool code generation
- [x] Tool compilation and execution sandbox
- [x] Effectiveness tracking
- [x] Tool evolution system

**Key Files**:
- `backend/src/synthesis/detector.rs` - Pattern detection
- `backend/src/synthesis/generator.rs` - Code generation
- `backend/src/synthesis/evolver.rs` - Tool evolution
- `backend/src/synthesis/storage.rs` - Persistence
- `backend/src/synthesis/loader.rs` - Tool loading
- `backend/src/synthesis/types.rs` - Type definitions

**Commit**: `baf6f83`

### Milestone 5: Build System Integration - COMPLETE

**Goal**: Error tracking and fix learning

**Deliverables**:
- [x] Build/test execution tracking
- [x] Error parsing (cargo, npm, pytest, etc.)
- [x] Error deduplication
- [x] Resolution tracking
- [x] Auto-inject recent errors into context
- [x] Historical fix lookup

**Key Files**:
- `backend/src/build/runner.rs` - Build execution
- `backend/src/build/parser.rs` - Error parsing
- `backend/src/build/tracker.rs` - Error tracking
- `backend/src/build/resolver.rs` - Fix resolution
- `backend/src/build/types.rs` - Type definitions

**Commit**: `19b7a95`

### Milestone 6: Reasoning Patterns - COMPLETE

**Goal**: Learn and replay successful patterns

**Deliverables**:
- [x] Pattern storage
- [x] Pattern matching (LLM-based)
- [x] Pattern replay
- [x] Success rate tracking
- [x] Pattern evolution

**Key Files**:
- `backend/src/patterns/storage.rs` - Pattern persistence
- `backend/src/patterns/matcher.rs` - Pattern matching
- `backend/src/patterns/replay.rs` - Pattern replay
- `backend/src/patterns/types.rs` - Type definitions

**Commit**: `fdc561c`

### Milestone 7: Context Oracle Integration - COMPLETE

**Goal**: Unified context gathering using all intelligence systems

**Deliverables**:
- [x] Context Oracle module with 8 intelligence sources
- [x] AppState integration with all services
- [x] OperationEngine integration
- [x] ContextBuilder integration with oracle output
- [x] Enhanced RecallEngine combining oracle + memory
- [x] Budget-aware context config selection
- [x] End-to-end testing with real LLM (Gemini 3 Pro)

**Key Files**:
- `backend/src/context_oracle/types.rs` - Context types and configs
- `backend/src/context_oracle/gatherer.rs` - Main gathering logic (675 lines)
- `backend/src/state.rs` - Service initialization
- `backend/src/operations/engine/context.rs` - Oracle integration in context building
- `backend/src/operations/engine/mod.rs` - OperationEngine with oracle
- `backend/src/memory/features/recall_engine/mod.rs` - RecallEngine with oracle
- `backend/src/memory/service/mod.rs` - MemoryService with oracle

**Intelligence Sources**:
1. Code context (semantic search)
2. Call graph (callers/callees)
3. Co-change suggestions (files changed together)
4. Historical fixes (similar past fixes)
5. Design patterns (detected patterns)
6. Reasoning patterns (suggested approaches)
7. Build errors (recent errors)
8. Expertise (author expertise)

**RecallEngine Integration**:
- `RecallContext` now includes `code_intelligence: Option<GatheredContext>`
- `build_enriched_context()` combines memory + oracle in single call
- `MemoryService::with_oracle()` for oracle-enabled memory service
- 20 tests in `recall_engine_oracle_test.rs`

**Budget-Aware Config Selection**:
- `ContextConfig::for_budget(daily%, monthly%)` - auto-selects minimal/standard/full
- `ContextConfig::for_error_with_budget()` - error-focused config respecting budget
- `BudgetStatus` struct with `get_config()`, `is_critical()`, `is_low()`, `daily_remaining()`, `monthly_remaining()`
- `BudgetTracker::get_budget_status()` - queries DB for current usage

**E2E Testing**:
- 4 integration tests in `context_oracle_e2e_test.rs`
- Tests full flow: Oracle + MemoryService + BudgetTracker
- Requires real Google API key and Qdrant (run with `--ignored`)

**Commits**:
- `678998d` - Integrate Context Oracle into AppState and OperationEngine

### Milestone 8: Real-time File Watching - COMPLETE

**Goal**: Replace polling-based file sync with real-time file watching

**Deliverables**:
- [x] File watcher service using notify crate (cross-platform)
- [x] Debounced event processing (300ms per-file, 1000ms batch)
- [x] Git operation cooldown (3s) to prevent redundant processing
- [x] Content hash comparison to skip unchanged files
- [x] Audit trail logging to local_changes table
- [x] Automatic registration of existing repositories at startup
- [x] Gap fixes: file deletion detection, collection-aware embedding cleanup

**Key Files**:
- `backend/src/watcher/mod.rs` - WatcherService orchestration
- `backend/src/watcher/config.rs` - Environment-based configuration
- `backend/src/watcher/events.rs` - FileChangeEvent types
- `backend/src/watcher/registry.rs` - WatchRegistry for managing paths
- `backend/src/watcher/processor.rs` - EventProcessor for file changes
- `backend/src/tasks/mod.rs` - TaskManager integration
- `backend/src/tasks/config.rs` - TASK_FILE_WATCHER_ENABLED flag
- `backend/src/tasks/code_sync.rs` - File deletion detection fix
- `backend/src/tasks/embedding_cleanup.rs` - Collection-aware cleanup fix

**Configuration**:
- `TASK_FILE_WATCHER_ENABLED=true` - Enable/disable watcher (default: true)
- `WATCHER_DEBOUNCE_MS=300` - Per-file debounce delay
- `WATCHER_BATCH_MS=1000` - Batch collection window
- `WATCHER_GIT_COOLDOWN_MS=3000` - Post-git-operation cooldown

**Dependencies Added**:
- `notify` v8 - Cross-platform file system notifications
- `notify-debouncer-full` v0.5 - Event debouncing

### Milestone 9: Frontend Integration

**Goal**: UI for all new features

**Deliverables**:
- [x] Semantic search UI (SemanticSearch.tsx)
- [x] Co-change suggestions panel (CoChangeSuggestions.tsx)
- [x] Tool synthesis dashboard (ToolsDashboard.tsx)
- [x] Budget tracking UI (BudgetTracker.tsx)
- [x] Build error integration (BuildErrorsPanel.tsx)
- [x] Enhanced file browser with semantic tags (FileBrowser.tsx)
- [x] Git-style diff viewing for artifacts (UnifiedDiffView.tsx)

**Files**:
- frontend/src/components/SemanticSearch.tsx
- frontend/src/components/CoChangeSuggestions.tsx
- frontend/src/components/ToolsDashboard.tsx
- frontend/src/components/BuildErrorsPanel.tsx
- frontend/src/components/BudgetTracker.tsx
- frontend/src/components/UnifiedDiffView.tsx
- frontend/src/components/FileBrowser.tsx (enhanced)
- frontend/src/stores/useCodeIntelligenceStore.ts

### Milestone 10: Production Hardening - COMPLETE

**Goal**: Performance, reliability, documentation

**Deliverables**:
- [x] Health check endpoints (`/health`, `/ready`, `/live`)
- [x] Graceful shutdown (SIGTERM handling)
- [x] Rate limiting integration
- [x] Prometheus metrics endpoint (`/metrics`)
- [x] Performance optimization (parallelized context/search)
- [x] Error handling improvements (264 unwraps fixed)
- [x] Comprehensive logging (754 tracing calls)
- [x] Deployment documentation
- [x] User guide
- [x] API documentation

**Key Files**:
- `backend/src/api/http/health.rs` - Health check endpoints
- `backend/src/metrics/mod.rs` - Prometheus metrics
- `backend/src/config/server.rs` - Rate limit configuration
- `backend/src/main.rs` - Graceful shutdown signal handler

**Tests**:
- Load testing
- Cache performance benchmarks
- Budget tracking accuracy
- End-to-end user workflows

### Milestone 11: Claude Code Feature Parity - COMPLETE

**Goal**: Implement key Claude Code features for improved developer experience

**Deliverables**:
- [x] Custom Slash Commands (`src/commands/mod.rs`)
- [x] Hooks System (`src/hooks/mod.rs`)
- [x] Checkpoint/Rewind System (`src/checkpoint/mod.rs`)
- [x] MCP Support (`src/mcp/`)

**Key Files**:
- `backend/src/commands/mod.rs` - CommandRegistry with markdown file loading
- `backend/src/hooks/mod.rs` - HookManager with pre/post tool execution
- `backend/src/checkpoint/mod.rs` - CheckpointManager with file state snapshots
- `backend/src/mcp/mod.rs` - McpManager with JSON-RPC 2.0 protocol
- `backend/src/mcp/protocol.rs` - JSON-RPC message types
- `backend/src/mcp/transport.rs` - Stdio transport for MCP servers

**Built-in Commands**:
- `/commands` - List available slash commands
- `/reload-commands` - Hot-reload commands from disk
- `/checkpoints` - List session checkpoints
- `/rewind <id>` - Restore files to checkpoint state
- `/mcp` - List MCP servers and tools

**Configuration Files**:
- `.mira/commands/*.md` - Project slash commands
- `~/.mira/commands/*.md` - User slash commands
- `.mira/hooks.json` - Hook configuration
- `.mira/mcp.json` - MCP server configuration

**Features**:

1. **Slash Commands**:
   - Markdown files with `$ARGUMENTS` placeholder
   - Project and user scopes
   - Recursive namespacing (`git/pr.md` -> `/git:pr`)
   - Description extraction from headers

2. **Hooks System**:
   - PreToolUse/PostToolUse triggers
   - Pattern matching with wildcards (`write_*`)
   - OnFailure modes: Block, Warn, Ignore
   - Environment variables for tool context

3. **Checkpoint/Rewind**:
   - Automatic snapshots before file modifications
   - SHA-256 content hashing
   - Session-scoped storage
   - Partial ID matching for restore

4. **MCP Support**:
   - JSON-RPC 2.0 over stdio
   - Tool discovery and execution
   - OpenAI-compatible format conversion
   - Background server connection

**Tests**: 25 new tests (3 commands + 7 hooks + 5 checkpoint + 10 MCP)

**Commits**:
- `8da8201` - Slash commands system
- `ff4e573` - Hooks system core module
- `d11f054` - Hooks integration
- `4f84712` - Checkpoint/Rewind system
- `0ae0431` - MCP support

## Success Metrics

### Technical Metrics
- **Cache Hit Rate**: 80%+ (target: 85%)
- **Test Coverage**: 80%+ for all new modules
- **Query Performance**: Semantic search < 200ms (p95)
- **Build Error Detection**: 95%+ of compile errors tracked
- **Pattern Detection Accuracy**: 90%+ precision for design patterns

### Cost Metrics
- **Daily Spending**: < $5/user/day (with caching)
- **Monthly Spending**: < $150/user/month
- **Cache Savings**: 80%+ cost reduction vs no caching
- **Budget Compliance**: 100% of users within limits

### User Experience Metrics
- **Context Relevance**: 90%+ of suggested files are relevant
- **Co-change Accuracy**: 85%+ of co-change suggestions are valid
- **Tool Synthesis Success**: 3+ useful tools per project
- **Pattern Replay Success**: 90%+ of pattern applications succeed

## Technology Stack

### Backend
- **Language**: Rust 1.91 (edition 2024)
- **Web Framework**: Axum + Tower
- **WebSocket**: tokio-tungstenite
- **Database**: SQLite 3.35+ with sqlx
- **Vector DB**: Qdrant (localhost:6334 gRPC)
- **LLM**: Gemini 3 Pro via Google AI API
- **Embeddings**: gemini-embedding-001 via Google AI API

### Frontend
- **Framework**: React 18+ with TypeScript
- **Build Tool**: Vite
- **State Management**: Zustand
- **Editor**: Monaco Editor
- **UI Components**: Custom + Headless UI

### Infrastructure
- **Development**: Local development with hot reload
- **Production**: Docker containers (backend + Qdrant)
- **Database**: SQLite with WAL mode
- **Caching**: In-process LLM cache (SQLite)
- **Authentication**: JWT tokens

## Future Enhancements (Post-V1)

### Deferred Features
1. ~~**Proactive Monitoring** - Background file watching with suggestions~~ (Implemented in Milestone 8)
2. **LSP Integration** - Type information from language servers
3. **Remote Development** - SSH support for remote codebases
4. **Multi-language AST** - Beyond Rust and TypeScript
5. **Collaborative Features** - Real-time co-editing
6. **Team Analytics** - Team-wide expertise tracking
7. **CI/CD Integration** - Hook into build pipelines
8. **IDE Extensions** - VSCode, IntelliJ, etc.

### Research Directions
1. **Multi-modal Code Understanding** - Images, diagrams, architecture docs
2. **Cross-project Learning** - Learn patterns across user's projects
3. **Automated Refactoring** - Safe, large-scale refactoring
4. **Test Generation** - Comprehensive test suites from code
5. **Documentation Generation** - Auto-generate docs from code + comments

## Contributing

### Getting Started
1. Clone repository
2. Install Rust 1.91+ and Node.js 18+
3. Start Qdrant (Docker: `docker run -p 6333:6333 -p 6334:6334 qdrant/qdrant`)
4. Configure `.env` (see backend/.env.example)
5. Run migrations: `cd backend && sqlx migrate run`
6. Start backend: `cd backend && cargo run`
7. Start frontend: `cd frontend && npm run dev`

### Development Workflow
1. Create feature branch
2. Write tests first
3. Implement feature
4. Run tests: `cargo test` and `npm run test`
5. Check formatting: `cargo fmt` and `npm run lint`
6. Submit PR with description

### Code Style
- Follow CLAUDE.md guidelines
- No emojis in code/docs
- File headers with path comments
- Concise, focused comments
- Prefer editing existing files over creating new ones

## Status

**Last Updated**: 2025-12-05
**Current Phase**: All milestones complete
**Owner**: Peter (Founder)

### Completed Milestones

**Milestone 1: Foundation** - Complete
- 9 SQL migrations (50+ tables)
- Gemini 3 Pro provider with variable thinking levels
- Budget tracking and LLM cache modules
- 3 Qdrant collections (code, conversation, git)
- 127+ tests passing

**Milestone 2: Code Intelligence** - Complete
- Semantic graph with purpose/concepts/domain labels
- Call graph with impact analysis
- Design pattern detection (Factory, Repository, Builder, etc.)
- Domain clustering with cohesion scoring
- Analysis caching with LFU eviction

**Milestone 3: Git Intelligence** - Complete
- Commit tracking with file changes
- Co-change pattern detection (Jaccard confidence)
- Blame annotations with cache invalidation
- Author expertise scoring (40/30/30 formula)
- Historical fix matching with error normalization

**Milestone 4: Tool Synthesis** - Complete
- Tool pattern detection (LLM-based)
- Tool code generation and execution sandbox
- Effectiveness tracking and evolution system
- Commit: `baf6f83`

**Milestone 5: Build System Integration** - Complete
- Build/test execution tracking
- Error parsing (cargo, npm, pytest)
- Error deduplication and resolution tracking
- Commit: `19b7a95`

**Milestone 6: Reasoning Patterns** - Complete
- Pattern storage and matching
- Pattern replay with success rate tracking
- Commit: `fdc561c`

**Milestone 7: Context Oracle Integration** - Complete
- Unified context gathering with 8 intelligence sources
- Budget-aware config selection
- RecallEngine integration
- Commit: `678998d`

**Milestone 8: Real-time File Watching** - Complete
- Cross-platform file watcher (notify crate)
- Debounced event processing with batch collection
- Git operation cooldown, content hash comparison
- Gap fixes: deletion detection, collection-aware cleanup

**Milestone 9: Frontend Integration** - Complete
- Semantic search UI with real-time results
- Co-change suggestions panel with confidence scores
- Budget tracking UI with daily/monthly progress bars
- Git-style unified diff viewing for artifacts
- Build error integration panel (errors, builds, stats)
- Tool synthesis dashboard (tools, patterns, stats)
- Enhanced file browser with semantic tags (test/complexity/issues/analyzed)

**Milestone 10: Production Hardening** - Complete
- Health check endpoints (`/health`, `/ready`, `/live`)
- Graceful shutdown with SIGTERM handling
- Rate limiting integration (configurable via env)
- Prometheus metrics endpoint (`/metrics`)
- Error handling improvements (264 unwraps fixed)
- Comprehensive logging (754 tracing calls)

**Milestone 11: Claude Code Feature Parity** - Complete
- Custom slash commands
- Hooks system for pre/post tool execution
- Checkpoint/rewind system for file state snapshots
- MCP support for external tool integration

**Milestone 12: Agent System** - Complete
- Specialized agents (Claude Code-style)
- Agent manager with custom agent loading
- Tool router integration

### Next Steps

**Milestone 13: Multi-Model Routing** - IN PROGRESS

See detailed plan below.

---

## Milestone 13: Multi-Model Routing

**Status**: Phases 1-3 Complete, Phases 4-5 In Progress

### Goal

Implement Claude Code-style multi-model routing with four tiers (all OpenAI GPT-5.1 family):
- **Fast**: GPT-5.1 Mini ($0.25/$2) - File ops, search, simple tasks
- **Voice**: GPT-5.1 ($1.25/$10) - User-facing chat, personality, main interactions
- **Code**: GPT-5.1-Codex-Max ($1.25/$10, high reasoning) - Code generation, refactoring, complex reasoning
- **Agentic**: GPT-5.1-Codex-Max ($1.25/$10, xhigh reasoning) - Long-running autonomous tasks (24h+)

### Expected Outcome

| Metric | Current (Single Model) | After Multi-Model |
|--------|------------------------|-------------------|
| Simple task cost | $2-4/M tokens | $0.25/M tokens |
| Average cost/operation | ~$1-2 | ~$0.30-0.50 |
| Complex refactor | $9.24 | ~$0.50-1.00 |
| Long-running tasks | Not supported | 24h+ autonomous |

### Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│                            ModelRouter                                   │
├─────────────────────────────────────────────────────────────────────────┤
│  TaskClassifier → ModelTier → Provider Selection → Fallback Chain       │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
        ┌───────────────┬───────────┴───────────┬───────────────┐
        ▼               ▼                       ▼               ▼
┌───────────────┐ ┌───────────────┐ ┌─────────────────┐ ┌─────────────────┐
│  FAST TIER    │ │  VOICE TIER   │ │   CODE TIER     │ │  AGENTIC TIER   │
│  GPT-5.1 Mini │ │  GPT-5.1      │ │ GPT-5.1-Codex   │ │ GPT-5.1-Codex   │
│  $0.25/$2/M   │ │  $1.25/$10/M  │ │ Max (high)      │ │ Max (xhigh)     │
│  file ops     │ │  user chat    │ │ code tasks      │ │ long-running    │
└───────────────┘ └───────────────┘ └─────────────────┘ └─────────────────┘

Fallback Chain: Fast → Voice → Code → Agentic
```

### Phase 1: OpenAI Provider (Foundation) ✓ COMPLETE

**Goal**: Add OpenAI as a second LLM provider alongside Gemini

**Files Created**:
- `backend/src/llm/provider/openai/mod.rs` - OpenAI provider module
- `backend/src/llm/provider/openai/types.rs` - Request/response types, ReasoningEffort enum
- `backend/src/llm/provider/openai/conversion.rs` - Message format conversion
- `backend/src/llm/provider/openai/pricing.rs` - GPT-5.1 family pricing
- `backend/src/llm/provider/openai/embeddings.rs` - OpenAI embeddings (text-embedding-3-large)

**Implementation**:

```rust
// backend/src/llm/provider/openai/types.rs
pub enum OpenAIModel {
    Gpt51,          // gpt-5.1 - Voice tier
    Gpt51Mini,      // gpt-5.1-mini - Fast tier
    Gpt51CodexMax,  // gpt-5.1-codex-max - Code/Agentic tiers
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningEffort {
    Medium,  // Default for Voice
    High,    // Code tier
    XHigh,   // Agentic tier (24h+ tasks)
}

// backend/src/llm/provider/openai/mod.rs
impl OpenAIProvider {
    pub fn gpt51_mini(api_key: String) -> Result<Self> { ... }
    pub fn gpt51(api_key: String) -> Result<Self> { ... }
    pub fn codex_max(api_key: String) -> Result<Self> { ... }        // high reasoning
    pub fn codex_max_agentic(api_key: String) -> Result<Self> { ... } // xhigh reasoning
}
```

**Deliverables**:
- [x] OpenAI provider implementing LlmProvider trait
- [x] Tool calling support (OpenAI format)
- [x] Streaming support
- [x] Pricing calculation for GPT-5.1, GPT-5.1 Mini, GPT-5.1-Codex-Max
- [x] ReasoningEffort parameter for Codex-Max (medium/high/xhigh)
- [x] Unit tests for provider

### Phase 2: Model Router ✓ COMPLETE

**Goal**: Intelligent routing of tasks to appropriate models

**Files Created**:
- `backend/src/llm/router/mod.rs` - Main router module with 4 providers
- `backend/src/llm/router/types.rs` - ModelTier enum, RoutingTask, RoutingStats
- `backend/src/llm/router/classifier.rs` - Task classification logic
- `backend/src/llm/router/config.rs` - Routing configuration from env

**Implementation**:

```rust
// backend/src/llm/router/types.rs
pub enum ModelTier {
    Fast,    // GPT-5.1 Mini - file ops, search
    Voice,   // GPT-5.1 - user chat (default)
    Code,    // GPT-5.1-Codex-Max (high) - code tasks
    Agentic, // GPT-5.1-Codex-Max (xhigh) - long-running 24h+
}

pub struct RoutingTask {
    pub tool_name: Option<String>,
    pub operation_kind: Option<String>,
    pub estimated_tokens: i64,
    pub file_count: usize,
    pub is_user_facing: bool,
    pub is_long_running: bool,  // Routes to Agentic tier
    pub tier_override: Option<ModelTier>,
}

// backend/src/llm/router/mod.rs
pub struct ModelRouter {
    fast_provider: Arc<dyn LlmProvider>,    // GPT-5.1 Mini
    voice_provider: Arc<dyn LlmProvider>,   // GPT-5.1
    code_provider: Arc<dyn LlmProvider>,    // GPT-5.1-Codex-Max (high)
    agentic_provider: Arc<dyn LlmProvider>, // GPT-5.1-Codex-Max (xhigh)
    classifier: TaskClassifier,
}
```

**Classification Rules**:

```rust
// backend/src/llm/router/classifier.rs
const FAST_TOOLS: &[&str] = &[
    "list_project_files", "search_codebase", "grep_files", "count_lines"
];

const CODE_OPERATIONS: &[&str] = &[
    "architecture", "refactor", "code_review", "test_generation",
    "implement_feature", "fix_bug", "security_audit"
];

const AGENTIC_OPERATIONS: &[&str] = &[
    "full_implementation", "migration", "large_refactor", "codebase_modernization"
];

// Routing priority:
// 1. Explicit override → use specified tier
// 2. is_long_running → Agentic
// 3. AGENTIC_OPERATIONS → Agentic
// 4. User-facing chat → Voice
// 5. FAST_TOOLS → Fast
// 6. CODE_OPERATIONS → Code
// 7. >50k tokens or >3 files → Code
// 8. Default → Voice
```

**Deliverables**:
- [x] ModelRouter with 4 provider tiers
- [x] TaskClassifier with comprehensive heuristics
- [x] Configuration via .env (MODEL_FAST, MODEL_VOICE, MODEL_CODE, MODEL_AGENTIC)
- [x] Fallback chain (Fast → Voice → Code → Agentic)
- [x] RoutingStats with per-tier metrics and cost tracking

### Phase 3: Integration ✓ COMPLETE

**Goal**: Wire router into existing operation engine

**Files Modified**:
- `backend/src/state.rs` - Initialize 4 providers and router
- `backend/src/operations/engine/llm_orchestrator.rs` - Use router for provider selection
- `backend/src/budget/mod.rs` - Track costs per provider with tier info

**Implementation**:

```rust
// backend/src/state.rs
impl AppState {
    pub async fn new(pool: SqlitePool) -> Result<Self> {
        // Initialize 4 OpenAI providers
        let fast_provider = Arc::new(OpenAIProvider::gpt51_mini(...));
        let voice_provider = Arc::new(OpenAIProvider::gpt51(...));
        let code_provider = Arc::new(OpenAIProvider::codex_max(...));
        let agentic_provider = Arc::new(OpenAIProvider::codex_max_agentic(...));

        let model_router = Arc::new(ModelRouter::new(
            fast_provider,
            voice_provider,
            code_provider,
            agentic_provider,
            RouterConfig::from_env(),
        ));
        // ...
    }
}

// backend/src/operations/engine/llm_orchestrator.rs
impl LlmOrchestrator {
    async fn record_cost(&self, tier: ModelTier, tokens_input: i64, tokens_output: i64) {
        let (model, provider_name, model_name) = match tier {
            ModelTier::Fast => (OpenAIModel::Gpt51Mini, "openai", "gpt-5.1-mini"),
            ModelTier::Voice => (OpenAIModel::Gpt51, "openai", "gpt-5.1"),
            ModelTier::Code => (OpenAIModel::Gpt51CodexMax, "openai", "gpt-5.1-codex-max"),
            ModelTier::Agentic => (OpenAIModel::Gpt51CodexMax, "openai", "gpt-5.1-codex-max"),
        };
        let cost = OpenAIPricing::calculate_cost(model, tokens_input, tokens_output);
        self.budget_tracker.record_request(..., tier.as_str(), ...).await?;
    }
}
```

**Deliverables**:
- [x] Router integration in AppState (4 providers)
- [x] LlmOrchestrator using router for tier-based cost tracking
- [x] Per-provider cost tracking with tier metadata
- [x] Logging of routing decisions (via tracing)
- [x] Graceful degradation via fallback chain

### Phase 4: Voice & Personality (COMPLETE)

**Goal**: Ensure Mira maintains consistent personality through GPT-5.1 Voice tier

**Files Modified**:
- `backend/src/persona/default.rs` - Streamlined persona for GPT-5.1
- `backend/src/prompt/context.rs` - Optimized context builders
- `backend/src/llm/router/classifier.rs` - Verified Voice tier for user chat

**Implementation Details** (Completed):

The Voice tier (GPT-5.1) handles all direct user interactions:
- Chat responses
- Explanations
- Status updates
- Error messages

```rust
// Explicit Voice tier for user-facing chat
let response = model_router
    .voice()  // Always use Voice tier for user chat
    .chat(messages, system_prompt)
    .await?;
```

Mira's personality should be consistent:
- Helpful and knowledgeable
- Concise but thorough
- Remembers user preferences (via memory system)
- Professional tone

**Deliverables**:
- [ ] Chat handler explicitly uses `model_router.voice()` for user messages
- [ ] System prompt optimized for GPT-5.1
- [ ] Personality consistency tests
- [ ] Ensure Code/Agentic tiers don't leak into user-facing chat

### Phase 5: Testing & Validation

**Goal**: Comprehensive testing of multi-model system

**Existing Tests** (in library):
- `llm::router::tests` - Router unit tests (7 tests)
- `llm::router::classifier::tests` - Classifier tests (8 tests)
- `llm::router::config::tests` - Config tests (2 tests)
- `llm::provider::openai::tests` - Provider tests (4 tests)

**Files to Create**:
- `backend/tests/multi_model_e2e_test.rs` - End-to-end routing tests

**Test Cases**:
1. **Classification tests**: Verify correct tier assignment ✓
2. **Provider tests**: Each provider works independently ✓
3. **Routing tests**: Tasks go to correct providers ✓
4. **Fallback tests**: Graceful degradation ✓
5. **Cost tests**: Verify cost savings vs single model
6. **Personality tests**: Voice consistency
7. **Agentic tests**: Long-running task handling

**Validation Metrics**:
- [ ] Fast tier used for 60%+ of tool calls
- [ ] Cost reduction of 70%+ on file operations
- [ ] No personality drift in user chat
- [ ] Latency acceptable (<2s for Fast, <5s for Voice, <30s for Code/Agentic)

### Configuration

**Environment Variables**:

```bash
# backend/.env additions

# OpenAI Configuration
OPENAI_API_KEY=sk-...

# Model Routing (4 tiers, all OpenAI)
MODEL_ROUTER_ENABLED=true
MODEL_FAST=gpt-5.1-mini
MODEL_VOICE=gpt-5.1
MODEL_CODE=gpt-5.1-codex-max
MODEL_AGENTIC=gpt-5.1-codex-max

# Routing Thresholds (upgrade to Code tier)
ROUTE_CODE_TOKEN_THRESHOLD=50000
ROUTE_CODE_FILE_COUNT=3

# Optional: Logging
MODEL_ROUTER_LOG=true
MODEL_ROUTER_FALLBACK=true
```

### Migration Strategy

**Rollout Plan**:
1. ✓ Deploy with `MODEL_ROUTER_ENABLED=true` (4-tier routing active)
2. ✓ All tiers using OpenAI GPT-5.1 family
3. Monitor routing decisions via logs
4. Tune thresholds based on usage patterns

**Rollback**:
- Set `MODEL_ROUTER_ENABLED=false` to use Voice tier for everything

### Cost Projections

**Current (4-Tier OpenAI)**:

| Task Type | Tier | Cost |
|-----------|------|------|
| File list (10k tokens) | Fast | ~$0.02 |
| Simple chat | Voice | ~$0.10 |
| Code refactor (50k tokens) | Code | ~$0.50-1 |
| Full implementation | Agentic | ~$1-3 |
| Full session | Mixed | ~$1-5 |

**GPT-5.1-Codex-Max Benefits**:
- Context: Millions of tokens via compaction
- Max output: 128k tokens
- SWE-Bench: 77.9% (highest)
- Long-running: 24h+ autonomous tasks

**Savings**: 60-80% cost reduction vs using Code tier for everything

### Timeline Estimate

| Phase | Scope | Status |
|-------|-------|--------|
| Phase 1: OpenAI Provider | New provider, types, tests | ✓ Complete |
| Phase 2: Model Router | Router, classifier, config | ✓ Complete |
| Phase 3: Integration | Wire into existing code | ✓ Complete |
| Phase 4: Voice & Personality | Prompt tuning, chat routing | Complete |
| Phase 5: Testing | Tests, validation, metrics | Pending |

### Dependencies

**New Crates** (if needed):
- None - reuse existing `reqwest` for HTTP

**API Keys Required**:
- `OPENAI_API_KEY` - OpenAI API access (for all 4 tiers)

### Success Criteria

- [x] All 4 providers working independently
- [x] Router correctly classifying tasks (21 unit tests passing)
- [ ] Cost reduction of 60%+ on typical sessions (pending validation)
- [ ] No degradation in response quality
- [ ] Mira personality consistent across interactions
- [x] Graceful fallback on provider failures

### Architecture

Gemini 3 Pro single-model with variable thinking levels (low/high). Database schema combines mira-cli's programming context oracle with Mira's personal memory system. All operations routed through the unified Operation Engine. Real-time file watching via notify crate for immediate code intelligence updates.
