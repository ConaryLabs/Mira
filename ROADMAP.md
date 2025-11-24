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

### Milestone 1: Foundation (Weeks 1-2) - CURRENT

**Goal**: Core schema, GPT 5.1 integration, budget/cache system

**Deliverables**:
- [x] 9 SQL migrations (50+ tables)
- [ ] 3 Qdrant collections setup
- [ ] GPT 5.1 provider implementation
- [ ] Budget tracking module
- [ ] LLM cache module
- [ ] Basic tests

**Files**:
- backend/migrations/20251125_001_foundation.sql
- backend/migrations/20251125_002_code_intelligence.sql
- backend/migrations/20251125_003_git_intelligence.sql
- backend/migrations/20251125_004_operations.sql
- backend/migrations/20251125_005_documents.sql
- backend/migrations/20251125_006_tool_synthesis.sql
- backend/migrations/20251125_007_build_system.sql
- backend/migrations/20251125_008_budget_cache.sql
- backend/migrations/20251125_009_project_context.sql
- backend/src/llm/provider/gpt5.rs
- backend/src/budget/mod.rs
- backend/src/cache/mod.rs

### Milestone 2: Code Intelligence (Weeks 3-4)

**Goal**: Semantic graph, call graph, pattern detection

**Deliverables**:
- Semantic node/edge management
- Call graph traversal
- Design pattern detection (LLM-based)
- Domain clustering
- Code quality analysis
- Semantic analysis cache

**Files**:
- backend/src/memory/features/code_intelligence/semantic.rs
- backend/src/memory/features/code_intelligence/call_graph.rs
- backend/src/memory/features/code_intelligence/patterns.rs
- backend/src/memory/features/code_intelligence/clustering.rs
- backend/src/memory/features/code_intelligence/cache.rs

**Tests**:
- backend/tests/code_intelligence_test.rs
- backend/tests/semantic_graph_test.rs
- backend/tests/call_graph_test.rs

### Milestone 3: Git Intelligence (Weeks 5-6)

**Goal**: Commit tracking, co-change analysis, expertise scoring

**Deliverables**:
- Git commit indexing
- Co-change pattern detection (LLM-based)
- Blame annotation management
- Author expertise scoring
- Historical fix matching
- Git context queries

**Files**:
- backend/src/git/intelligence/commits.rs
- backend/src/git/intelligence/cochange.rs
- backend/src/git/intelligence/blame.rs
- backend/src/git/intelligence/expertise.rs
- backend/src/git/intelligence/fixes.rs

**Tests**:
- backend/tests/git_intelligence_test.rs
- backend/tests/cochange_detection_test.rs

### Milestone 4: Tool Synthesis (Weeks 7-8)

**Goal**: Auto-generate custom tools from codebase patterns

**Deliverables**:
- Tool pattern detection (LLM-based)
- Tool code generation
- Tool compilation and execution sandbox
- Effectiveness tracking
- Tool evolution system

**Files**:
- backend/src/synthesis/detector.rs
- backend/src/synthesis/generator.rs
- backend/src/synthesis/executor.rs
- backend/src/synthesis/evolution.rs

**Tests**:
- backend/tests/tool_synthesis_test.rs
- backend/tests/tool_execution_test.rs

### Milestone 5: Build System Integration (Weeks 9-10)

**Goal**: Error tracking and fix learning

**Deliverables**:
- Build/test execution tracking
- Error parsing (cargo, npm, pytest, etc.)
- Error deduplication
- Resolution tracking
- Auto-inject recent errors into context
- Historical fix lookup

**Files**:
- backend/src/build/runner.rs
- backend/src/build/parser.rs
- backend/src/build/tracker.rs
- backend/src/build/resolver.rs

**Tests**:
- backend/tests/build_system_test.rs
- backend/tests/error_resolution_test.rs

### Milestone 6: Reasoning Patterns (Weeks 11-12)

**Goal**: Learn and replay successful patterns

**Deliverables**:
- Pattern storage
- Pattern matching (LLM-based)
- Pattern replay
- Success rate tracking
- Pattern evolution

**Files**:
- backend/src/patterns/storage.rs
- backend/src/patterns/matching.rs
- backend/src/patterns/replay.rs

**Tests**:
- backend/tests/reasoning_patterns_test.rs
- backend/tests/pattern_matching_test.rs

### Milestone 7: Context Oracle Integration (Weeks 13-14)

**Goal**: Unified context gathering using all intelligence systems

**Deliverables**:
- Enhanced RecallEngine with semantic graph
- Call graph context gathering
- Co-change file suggestions
- Historical fix integration
- Pattern-based context
- Budget-aware LLM calls

**Files**:
- backend/src/memory/features/recall_engine/enhanced.rs
- backend/src/memory/features/context_oracle.rs

**Tests**:
- backend/tests/context_oracle_test.rs
- backend/tests/end_to_end_context_test.rs

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
- **Language**: Rust 1.75+
- **Web Framework**: Axum + Tower
- **WebSocket**: tokio-tungstenite
- **Database**: SQLite 3.35+ with sqlx
- **Vector DB**: Qdrant (localhost:6333)
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
2. Install Rust 1.75+ and Node.js 18+
3. Install Qdrant (Docker: `docker run -p 6333:6333 qdrant/qdrant`)
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

**Last Updated**: 2025-11-25
**Status**: Foundation Phase - Schema Design Complete
**Owner**: Peter (Founder)

**Completed** (Nov 25, 2025):
- [x] 9 SQL migrations designed and created
- [x] Schema covers all features (code intelligence, git, tools, budget, patterns)
- [x] New ROADMAP.md created with unified vision

**Next Steps**:
1. Qdrant collection setup (3 collections: code, conversation, git)
2. GPT 5.1 provider implementation
3. Budget tracking and LLM cache modules
4. Update configuration files and documentation

**Architecture Change**: Complete pivot from DeepSeek dual-model to GPT 5.1 single-model with reasoning effort levels. Fresh database schema combining mira-cli's programming context oracle with Mira's personal memory system.
