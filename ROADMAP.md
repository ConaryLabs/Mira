# Mira Roadmap: Unified Programming + Personal Context Oracle

## Vision

Mira is a next-generation AI coding assistant combining:
- **Programming Context Oracle** from mira-cli (semantic code understanding, git intelligence, pattern detection)
- **Personal Memory System** from Mira (user preferences, communication style, learned patterns)
- **Web-based Interface** for accessibility and collaboration
- **GPT 5.1** for state-of-the-art reasoning with variable effort levels

The result is a well-rounded assistant that understands both your code and you as a developer.

## Core Architecture

### LLM Stack
- **Primary Model**: GPT 5.1 with variable reasoning effort (minimum/medium/high)
- **Embeddings**: OpenAI text-embedding-3-large (3072 dimensions)
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

**Goal**: Core schema, GPT 5.1 integration, budget/cache system

**Deliverables**:
- [x] 9 SQL migrations (50+ tables)
- [x] 3 Qdrant collections setup (code, conversation, git)
- [x] GPT 5.1 provider implementation
- [x] Budget tracking module
- [x] LLM cache module
- [x] Basic tests (127+ passing)

**Key Files**:
- `backend/migrations/` - 9 migration files
- `backend/src/llm/provider/gpt5.rs` - GPT 5.1 provider
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
- [x] End-to-end testing with real LLM (GPT 5.1)

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
- Requires real OpenAI API key and Qdrant (run with `--ignored`)

**Commits**:
- `678998d` - Integrate Context Oracle into AppState and OperationEngine

### Milestone 8: Frontend Integration (Weeks 15-18)

**Goal**: UI for all new features

**Deliverables**:
- Semantic search UI
- Co-change suggestions panel
- Tool synthesis dashboard
- Budget tracking UI
- Build error integration
- Enhanced file browser with semantic tags

**Files**:
- frontend/src/components/SemanticSearch.tsx
- frontend/src/components/CoChangeSuggestions.tsx
- frontend/src/components/ToolsDashboard.tsx
- frontend/src/components/BudgetTracker.tsx
- frontend/src/stores/useCodeIntelligenceStore.ts

### Milestone 9: Production Hardening (Weeks 19-20)

**Goal**: Performance, reliability, documentation

**Deliverables**:
- Performance optimization
- Error handling improvements
- Comprehensive logging
- Deployment documentation
- User guide
- API documentation

**Tests**:
- Load testing
- Cache performance benchmarks
- Budget tracking accuracy
- End-to-end user workflows

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
- **LLM**: GPT 5.1 via OpenAI API
- **Embeddings**: text-embedding-3-large via OpenAI API

### Frontend
- **Framework**: React 18+ with TypeScript
- **Build Tool**: Vite
- **State Management**: Zustand
- **Editor**: Monaco Editor
- **Terminal**: xterm.js
- **UI Components**: Custom + Headless UI

### Infrastructure
- **Development**: Local development with hot reload
- **Production**: Docker containers (backend + Qdrant)
- **Database**: SQLite with WAL mode
- **Caching**: In-process LLM cache (SQLite)
- **Authentication**: JWT tokens

## Future Enhancements (Post-V1)

### Deferred Features
1. **Proactive Monitoring** - Background file watching with suggestions
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

**Last Updated**: 2025-11-26
**Current Phase**: Milestone 7 - Context Oracle Integration (In Progress)
**Owner**: Peter (Founder)

### Completed Milestones

**Milestone 1: Foundation** - Complete
- 9 SQL migrations (50+ tables)
- GPT 5.1 provider with variable reasoning effort
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

### Next Steps

1. **Milestone 7**: Context Oracle Integration - Unified context gathering using all intelligence systems
2. **Milestone 8**: Frontend Integration - UI for semantic search, co-change suggestions, tool dashboard
3. **Milestone 9**: Production Hardening - Performance, reliability, documentation

### Architecture

GPT 5.1 single-model with variable reasoning effort (minimum/medium/high). Database schema combines mira-cli's programming context oracle with Mira's personal memory system. All operations routed through the unified Operation Engine.
