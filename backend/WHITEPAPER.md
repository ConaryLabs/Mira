# Mira Backend Technical Whitepaper

This document provides a comprehensive technical reference for the Mira backend architecture, designed to help LLMs understand how the system works.

**Version:** 0.9.0
**Last Updated:** December 10, 2025
**Language:** Rust (Edition 2024)
**Lines of Code:** ~31,000

---

## Table of Contents

1. [System Overview](#1-system-overview)
2. [Architecture Diagram](#2-architecture-diagram)
3. [Module Directory Structure](#3-module-directory-structure)
4. [Database Schema](#4-database-schema)
5. [Core Modules & Responsibilities](#5-core-modules--responsibilities)
6. [LLM & Model Router](#6-llm--model-router)
7. [Memory Systems](#7-memory-systems)
8. [Operation Engine](#8-operation-engine)
9. [WebSocket Protocol](#9-websocket-protocol)
10. [Tools & Capabilities](#10-tools--capabilities)
11. [Configuration System](#11-configuration-system)
12. [Session Architecture](#12-session-architecture)
13. [CLI Architecture](#13-cli-architecture)
14. [Git Intelligence](#14-git-intelligence)
15. [Build System Integration](#15-build-system-integration)
16. [Testing Infrastructure](#16-testing-infrastructure)
17. [Dependencies & External Systems](#17-dependencies--external-systems)

---

## 1. System Overview

Mira is an AI-powered coding assistant with:

- **OpenAI GPT-5.1 Family** for all LLM operations with 4-tier routing
- **SQLite** for structured data storage (70+ tables)
- **Qdrant** for vector embeddings (3 collections)
- **WebSocket** for real-time client communication (port 3001)

### Key Design Principles

- **Multi-model routing**: 4 tiers (Fast/Voice/Code/Agentic) for cost optimization
- **Hybrid memory**: Structured (SQLite) + Semantic (Qdrant)
- **Dual-session**: Eternal Voice sessions + discrete Codex sessions
- **Comprehensive code intelligence**: Semantic graph, call graph, patterns
- **Budget tracking**: Daily/monthly limits with LLM response caching

---

## 2. Architecture Diagram

```
┌─────────────────────────────────────────────────────────────┐
│                      Client Layer                            │
│         React Frontend (5173) │ Rust CLI                    │
└───────────────────────────────┬─────────────────────────────┘
                                │ WebSocket
┌───────────────────────────────▼─────────────────────────────┐
│                    WebSocket Layer (3001)                    │
│                      src/api/ws/                            │
│    Handlers: chat, git, files, operations, session, sudo    │
└───────────────────────────────┬─────────────────────────────┘
                                │
┌───────────────────────────────▼─────────────────────────────┐
│                   Operation Engine                           │
│                 src/operations/engine/                       │
│    Orchestration, Tool Router, Context Building, Events     │
└───────────────────────────────┬─────────────────────────────┘
                                │
        ┌───────────────────────┼───────────────────────┐
        │                       │                       │
┌───────▼───────┐      ┌───────▼───────┐      ┌───────▼───────┐
│  Model Router │      │    Memory     │      │     Git       │
│   src/llm/    │      │   Service     │      │ Intelligence  │
│   router/     │      │  src/memory/  │      │   src/git/    │
│               │      │               │      │               │
│ 4-Tier:       │      │ Recall Engine │      │ Commits       │
│ Fast/Voice/   │      │ Summaries     │      │ Co-change     │
│ Code/Agentic  │      │ Embeddings    │      │ Expertise     │
└───────┬───────┘      └───────┬───────┘      └───────────────┘
        │                      │
┌───────▼───────┐      ┌───────┴───────┐
│   OpenAI      │      │               │
│  GPT-5.1      │  ┌───▼───┐    ┌──────▼──────┐
│  Family       │  │SQLite │    │   Qdrant    │
│               │  │70+ tbl│    │ 3 collections│
└───────────────┘  └───────┘    └─────────────┘
```

---

## 3. Module Directory Structure

```
backend/src/
├── agents/                      # Specialized agent system
│   ├── builtin/                 # Built-in agents (explore, plan, general)
│   ├── executor/                # Hybrid execution (in-process + subprocess)
│   ├── registry.rs              # Agent registration & loading
│   ├── types.rs                 # Agent definitions & configurations
│   └── protocol.rs              # Agent communication protocol
│
├── api/                         # API layer
│   ├── http/                    # HTTP endpoints (auth, health)
│   ├── ws/                      # WebSocket handlers
│   │   ├── chat/                # Chat handler + message routing
│   │   │   ├── unified_handler.rs   # Main message dispatcher
│   │   │   ├── connection.rs        # WebSocket lifecycle
│   │   │   ├── message_router.rs    # Route to handlers
│   │   │   └── heartbeat.rs         # Keep-alive pings
│   │   ├── code_intelligence.rs # Semantic search & AST queries
│   │   ├── documents.rs         # Document upload & processing
│   │   ├── files.rs             # File browser & operations
│   │   ├── filesystem.rs        # Filesystem access control
│   │   ├── git.rs               # Git operations (clone, diff, etc.)
│   │   ├── memory.rs            # Memory service access
│   │   ├── operations/          # Operation streaming & status
│   │   ├── project.rs           # Project management
│   │   ├── session.rs           # Session management
│   │   ├── sudo.rs              # Sudo approval & execution
│   │   └── message.rs           # Message type definitions
│   └── error.rs                 # Error types & handlers
│
├── auth/                        # Authentication & JWT
│   ├── jwt.rs                   # JWT token handling
│   ├── password.rs              # Password hashing (bcrypt)
│   ├── service.rs               # Auth service
│   └── models.rs                # Auth data models
│
├── bin/                         # Binary entry points
│   ├── mira.rs                  # CLI binary
│   └── mira_test.rs             # Scenario testing binary
│
├── budget/                      # Budget tracking
│   └── mod.rs                   # Daily/monthly limits & cost tracking
│
├── build/                       # Build system integration
│   ├── runner.rs                # Execute build commands
│   ├── parser.rs                # Parse build error output
│   ├── tracker.rs               # Store build runs & errors
│   ├── resolver.rs              # Learn from historical fixes
│   └── types.rs                 # Build-related types
│
├── cache/                       # LLM response caching
│   ├── mod.rs                   # SHA-256 based cache
│   ├── session_state.rs         # OpenAI prompt caching
│   └── session_state_store.rs   # Persistent cache state
│
├── checkpoint/                  # Checkpoint/rewind system
│   └── mod.rs                   # File state snapshots
│
├── cli/                         # Command-line interface
│   ├── repl.rs                  # Interactive REPL loop
│   ├── ws_client.rs             # WebSocket client
│   ├── session/                 # Session management
│   ├── project/                 # Project detection
│   ├── commands/                # Slash commands (builtin + custom)
│   │   ├── builtin.rs           # /resume, /review, /agents, etc.
│   │   └── mod.rs               # Command loading
│   ├── display/                 # Terminal output
│   └── args.rs                  # CLI argument parsing
│
├── config/                      # Configuration management
│   ├── mod.rs                   # Main config loader
│   ├── llm.rs                   # LLM configuration
│   ├── server.rs                # Server settings
│   ├── memory.rs                # Memory system settings
│   ├── caching.rs               # Caching configuration
│   └── tools.rs                 # Tool configuration
│
├── context_oracle/              # Unified context gathering
│   ├── gatherer.rs              # Assemble context from all systems
│   └── types.rs                 # Context type definitions
│
├── git/                         # Git integration
│   ├── client/                  # Git operations
│   │   ├── operations.rs        # Clone, checkout, commit
│   │   ├── project_ops.rs       # Project-level operations
│   │   ├── tree_builder.rs      # Build file tree
│   │   ├── diff_parser.rs       # Parse git diffs
│   │   └── code_sync.rs         # Sync code to Qdrant
│   ├── intelligence/            # Git intelligence
│   │   ├── commits.rs           # Commit tracking
│   │   ├── cochange.rs          # File co-change patterns
│   │   ├── blame.rs             # Git blame analysis
│   │   ├── expertise.rs         # Author expertise scoring
│   │   └── fixes.rs             # Historical fix learning
│   └── store.rs                 # Git data persistence
│
├── hooks/                       # Hook system
│   └── mod.rs                   # Pre/post tool execution hooks
│
├── llm/                         # LLM provider & routing
│   ├── provider/                # Provider implementations
│   │   ├── openai/              # OpenAI GPT-5.1 family
│   │   │   ├── mod.rs           # OpenAI API client
│   │   │   ├── types.rs         # Request/response types
│   │   │   ├── conversion.rs    # Format conversion
│   │   │   ├── pricing.rs       # Cost calculation
│   │   │   └── embeddings.rs    # text-embedding-3-large
│   │   ├── gemini3/             # Google Gemini 3 (alternative)
│   │   └── stream.rs            # Streaming response handling
│   ├── router/                  # 4-tier model router
│   │   ├── mod.rs               # Router orchestration
│   │   ├── classifier.rs        # Task classification
│   │   └── config.rs            # Router configuration
│   └── embeddings.rs            # Generic embedding interface
│
├── mcp/                         # Model Context Protocol
│   ├── mod.rs                   # MCP manager
│   ├── protocol.rs              # MCP protocol
│   └── transport.rs             # MCP transport
│
├── memory/                      # Hybrid memory system
│   ├── core/                    # Core abstractions
│   │   ├── traits.rs            # MemoryStore trait
│   │   └── types.rs             # MemoryEntry & structures
│   ├── features/                # Memory features
│   │   ├── code_intelligence/   # Semantic graph & analysis
│   │   │   ├── semantic.rs      # Semantic graph
│   │   │   ├── call_graph.rs    # Call graph analysis
│   │   │   ├── parser.rs        # Multi-language parser
│   │   │   ├── patterns.rs      # Design pattern detection
│   │   │   └── storage.rs       # Semantic storage
│   │   ├── document_processing/ # Document ingestion
│   │   ├── message_pipeline/    # Message analysis
│   │   ├── recall_engine/       # Context retrieval
│   │   │   ├── search/          # Search strategies
│   │   │   │   ├── hybrid_search.rs
│   │   │   │   ├── semantic_search.rs
│   │   │   │   ├── recent_search.rs
│   │   │   │   └── multihead_search.rs
│   │   │   ├── scoring/         # Result scoring
│   │   │   └── context/         # Context assembly
│   │   └── summarization/       # Message summarization
│   │       └── strategies/      # Rolling, snapshot
│   ├── service/                 # Memory service coordination
│   │   └── core_service.rs      # Main memory service
│   └── storage/                 # Persistence layer
│       ├── sqlite/              # SQLite implementation
│       └── qdrant/              # Qdrant vector store
│
├── operations/                  # Operation orchestration
│   ├── engine/                  # Core operation logic
│   │   ├── orchestration.rs     # Main execution loop
│   │   ├── lifecycle.rs         # State transitions
│   │   ├── delegation.rs        # Task delegation
│   │   ├── llm_orchestrator.rs  # LLM interaction
│   │   ├── events.rs            # Event emission
│   │   ├── artifacts.rs         # Artifact handling
│   │   ├── context.rs           # Context assembly
│   │   ├── code_handlers.rs     # Code operations
│   │   ├── file_handlers.rs     # File operations
│   │   ├── git_handlers.rs      # Git operations
│   │   ├── external_handlers.rs # External tools
│   │   └── tool_router/         # Tool invocation
│   │       ├── registry.rs      # Tool registry
│   │       └── mod.rs           # Router orchestration
│   ├── tools/                   # Tool schema definitions
│   │   ├── code_generation.rs   # generate_code, refactor
│   │   ├── code_intelligence.rs # find_function, search
│   │   ├── file_operations.rs   # read_file, write_file
│   │   ├── git_analysis.rs      # git_log, cochanges
│   │   ├── external.rs          # web_search, run_command
│   │   └── agents.rs            # Agent tools
│   ├── delegation_tools.rs      # Tool calling interface
│   ├── tool_builder.rs          # Tool schema builder
│   └── types.rs                 # Operation types
│
├── patterns/                    # Reasoning pattern storage
│   ├── matcher.rs               # Pattern matching
│   ├── replay.rs                # Pattern replay
│   └── storage.rs               # Pattern persistence
│
├── project/                     # Project management
│   ├── store.rs                 # Project persistence
│   ├── guidelines.rs            # Guidelines loading
│   └── tasks/                   # Project tasks
│
├── prompt/                      # Prompt building
│   ├── builders.rs              # UnifiedPromptBuilder
│   ├── internal.rs              # Technical prompts
│   └── context.rs               # Context building
│
├── relationship/                # User context & facts
│   ├── service.rs               # Relationship service
│   ├── facts_service.rs         # Learned facts storage
│   └── pattern_engine.rs        # Pattern learning
│
├── session/                     # Dual-session architecture
│   ├── types.rs                 # Session types
│   ├── manager.rs               # Session management
│   ├── injection.rs             # Cross-session injection
│   ├── codex_spawner.rs         # Spawn Codex from Voice
│   ├── completion.rs            # Completion detection
│   └── summary_generator.rs     # Session summarization
│
├── state.rs                     # Global AppState (DI container)
│
├── sudo/                        # Sudo approval system
│   └── service.rs               # Permission checking
│
├── synthesis/                   # Tool synthesis
│   ├── detector.rs              # Pattern detection
│   ├── generator.rs             # Tool generation
│   └── storage.rs               # Tool persistence
│
├── system/                      # System environment
│   └── detector.rs              # Detect OS, shell, tools
│
├── tasks/                       # Background tasks
│   ├── backfill.rs              # Backfill missing data
│   ├── code_sync.rs             # Sync code to Qdrant
│   └── embedding_cleanup.rs     # Clean embeddings
│
├── utils/                       # Centralized utilities
│   ├── hash.rs                  # SHA-256 hashing
│   ├── rate_limiter.rs          # Rate limiting
│   ├── timeout.rs               # Timeout utilities
│   └── timestamp.rs             # Timestamp utilities
│
├── watcher/                     # File system watching
│   ├── config.rs                # Watcher configuration
│   ├── events.rs                # File system events
│   └── processor.rs             # Event processing
│
├── lib.rs                       # Library entry point
└── main.rs                      # Server entry point
```

---

## 4. Database Schema

The database consists of **70+ tables** across 4 migration files.

### 4.1 Foundation Tables (20251209000001)

**Users & Authentication:**
| Table | Purpose |
|-------|---------|
| `users` | User accounts (username, email, password_hash, preferences) |
| `sessions` | Authentication sessions (token, expires_at) |
| `user_profile` | Preferences & learning (languages, coding_style, tech_stack) |

**Projects & Files:**
| Table | Purpose |
|-------|---------|
| `projects` | Project metadata (name, path, language, framework, tags) |
| `git_repo_attachments` | Git repository links with sync status |
| `repository_files` | Indexed files (path, language, line_count, complexity) |
| `local_changes` | Track uncommitted file changes |

**Memory & Conversation:**
| Table | Purpose |
|-------|---------|
| `memory_entries` | All chat messages (session_id, role, content, timestamp) |
| `message_analysis` | Sentiment, intent, salience, error detection |
| `rolling_summaries` | Periodic summaries (every 100 messages) |
| `message_embeddings` | Track embeddings sent to Qdrant (entry_id → point_id) |

**Personal Context:**
| Table | Purpose |
|-------|---------|
| `memory_facts` | Learned user facts (preferences, habits) |
| `learned_patterns` | Behavioral patterns (trigger, success rate) |

**Chat Sessions:**
| Table | Purpose |
|-------|---------|
| `chat_sessions` | Conversation sessions (user_id, type, status, message_count) |
| `session_forks` | Fork relationships between sessions |
| `codex_session_links` | Voice ↔ Codex session mapping |
| `session_injections` | Cross-session message injection |
| `session_checkpoints` | Git commit tracking for sessions |

### 4.2 Code Intelligence Tables (20251209000002)

**AST & Symbols:**
| Table | Purpose |
|-------|---------|
| `code_elements` | Functions, classes, methods (name, type, complexity) |
| `call_graph` | Function call relationships (caller_id → callee_id) |
| `external_dependencies` | Import statements & dependencies |

**Semantic Graph:**
| Table | Purpose |
|-------|---------|
| `semantic_nodes` | Purpose/concepts for symbols |
| `semantic_edges` | Relationships between symbols |
| `concept_index` | Index concepts to symbols |
| `semantic_analysis_cache` | Cache AST analysis results |

**Design Patterns:**
| Table | Purpose |
|-------|---------|
| `design_patterns` | Detected patterns (MVC, singleton, observer) |
| `pattern_validation_cache` | Pattern detection caching |

**Domain & Quality:**
| Table | Purpose |
|-------|---------|
| `domain_clusters` | Group related symbols (cohesion_score) |
| `code_quality_issues` | Linting/analysis issues |
| `language_configs` | Parser configurations per language |

### 4.3 Operations Tables (20251209000003)

**Git Intelligence:**
| Table | Purpose |
|-------|---------|
| `git_commits` | Indexed commits (hash, author, message, changes) |
| `file_cochange_patterns` | Files changed together (confidence_score) |
| `blame_annotations` | Line-by-line git blame |
| `author_expertise` | Author skill scores per file pattern |
| `historical_fixes` | Learn from previous error fixes |

**Operations:**
| Table | Purpose |
|-------|---------|
| `operations` | Operation tracking (id, status, kind, tokens, cost) |
| `operation_events` | Status changes, LLM analysis, delegations |
| `operation_tasks` | Sub-tasks within operations |

**Artifacts:**
| Table | Purpose |
|-------|---------|
| `artifacts` | Generated code snippets (kind, content, applied) |
| `file_modifications` | Track file changes (original, modified, diff) |

**Documents:**
| Table | Purpose |
|-------|---------|
| `documents` | Uploaded documents (PDF, docx, etc.) |
| `document_chunks` | Chunked content with embeddings |

**Project Context:**
| Table | Purpose |
|-------|---------|
| `project_guidelines` | CLAUDE.md or similar files |
| `project_tasks` | Project-level tasks |
| `task_sessions` | Link sessions to tasks |

### 4.4 Infrastructure Tables (20251209000004)

**Build System:**
| Table | Purpose |
|-------|---------|
| `build_runs` | Build execution tracking |
| `build_errors` | Parsed errors from builds |
| `error_resolutions` | How errors were fixed |
| `build_context_injections` | Link errors to operations |

**Budget & Cache:**
| Table | Purpose |
|-------|---------|
| `budget_tracking` | Per-request cost tracking |
| `budget_summary` | Daily/monthly summaries |
| `llm_cache` | Response cache (key_hash, response, TTL) |
| `session_cache_state` | OpenAI prompt caching state |
| `session_file_hashes` | File content hashes sent to LLM |

**Reasoning Patterns:**
| Table | Purpose |
|-------|---------|
| `reasoning_patterns` | Stored reasoning chains |
| `reasoning_steps` | Steps within patterns |
| `pattern_usage` | Track pattern success |

**Tool Synthesis:**
| Table | Purpose |
|-------|---------|
| `tool_patterns` | Detected tool opportunities |
| `synthesized_tools` | Generated tool code |
| `tool_executions` | Tool execution history |
| `tool_effectiveness` | Tool success metrics |

**Checkpoints:**
| Table | Purpose |
|-------|---------|
| `checkpoints` | File state snapshots |
| `checkpoint_files` | Stored file content |

**Sudo System:**
| Table | Purpose |
|-------|---------|
| `sudo_permissions` | Allowed commands |
| `sudo_blocklist` | Dangerous commands (11 defaults) |
| `sudo_approval_requests` | Pending approvals |
| `sudo_audit_log` | Audit trail |

---

## 5. Core Modules & Responsibilities

### 5.1 AppState (`src/state.rs`)

Global dependency injection container with 30+ services:

```rust
pub struct AppState {
    pub sqlite_store: Arc<SqliteMemoryStore>,
    pub llm_provider: Arc<dyn LlmProvider>,
    pub memory_service: Arc<MemoryService>,
    pub operation_engine: Arc<OperationEngine>,
    pub context_oracle: Arc<ContextOracle>,
    pub model_router: Arc<ModelRouter>,
    pub session_manager: Arc<SessionManager>,
    pub budget_tracker: Arc<BudgetTracker>,
    pub llm_cache: Arc<LlmCache>,
    pub git_client: Arc<GitClient>,
    pub code_intelligence: Arc<CodeIntelligence>,
    pub sudo_service: Arc<SudoService>,
    // ... 20+ more services
}
```

### 5.2 API Layer (`src/api/`)

**HTTP Handlers:**
- `/health` - Readiness/liveness checks
- `/api/auth/login` - User authentication
- `/api/auth/register` - User registration

**WebSocket Handlers (`src/api/ws/`):**

| Handler | Responsibility |
|---------|---------------|
| `chat/unified_handler.rs` | Main message routing |
| `code_intelligence.rs` | Semantic search, symbol analysis |
| `documents.rs` | Document upload & processing |
| `files.rs` | File browser |
| `filesystem.rs` | Filesystem access with permissions |
| `git.rs` | Git operations (clone, diff, log) |
| `memory.rs` | Memory access & queries |
| `operations/` | Operation streaming & status |
| `project.rs` | Project CRUD |
| `session.rs` | Session management |
| `sudo.rs` | Sudo approval handling |

---

## 6. LLM & Model Router

### 6.1 4-Tier Model Architecture

| Tier | Model | Use Cases | Reasoning | Cost |
|------|-------|-----------|-----------|------|
| **Fast** | GPT-5.1-Codex-Mini | File ops, search, metadata | - | $0.25/$2 per 1M |
| **Voice** | GPT-5.1 | User chat, explanations | medium | $1.25/$10 per 1M |
| **Code** | GPT-5.1-Codex-Max | Code generation, >50k tokens | high | $1.25/$10 per 1M |
| **Agentic** | GPT-5.1-Codex-Max | Long-running (24h+) autonomous | xhigh | $1.25/$10 per 1M |

### 6.2 Routing Logic (`src/llm/router/classifier.rs`)

```rust
fn classify_task(prompt: &str, context: &OperationContext) -> ModelTier {
    // Decision tree:
    // - Simple file listing, grep, search → Fast
    // - User-facing chat, explanations → Voice
    // - Code generation, >50k tokens, >3 files → Code
    // - Autonomous task lasting >1hr → Agentic
}
```

### 6.3 Provider Implementation (`src/llm/provider/openai/`)

- Uses OpenAI Responses API (`/v1/responses`)
- Supports streaming via SSE
- Tool calling via OpenAI function format
- Embeddings via `text-embedding-3-large` (3072 dimensions)

---

## 7. Memory Systems

### 7.1 Dual-Store Architecture

- **SQLite**: Structured data (messages, analysis, metadata)
- **Qdrant**: Vector embeddings (semantic search)

### 7.2 Three Qdrant Collections

| Collection | Content | Dimensions |
|------------|---------|------------|
| `mira_code` | Code snippets & symbols | 3072 |
| `mira_conversation` | Chat history & summaries | 3072 |
| `mira_git` | Commits & blame | 3072 |

### 7.3 Memory Pipeline

```
Message Input
    ↓
Message Analysis (intent, salience, topics)
    ↓
Save to SQLite (memory_entries + analysis)
    ↓
Salience Check (MIN_SALIENCE > 0.5?)
    ↓
Generate Embeddings (text-embedding-3-large)
    ↓
Store in Qdrant (with metadata)
    ↓
Track in message_embeddings (entry_id → point_id)
```

### 7.4 Recall Strategy (Hybrid Search)

```
Query Input
    ↓
Recent Messages (LIMIT 10, ORDER BY timestamp DESC)
    ↓
Semantic Vector Search (MIRA_CONTEXT_SEMANTIC_MATCHES=10)
    ↓
Multi-head Search (semantic, code, documents, conversation)
    ↓
Composite Scoring (recency * 0.3 + relevance * 0.7)
    ↓
Rolling Summaries (if age > 168 hours)
    ↓
Final Context Bundle
```

### 7.5 Context Architecture

| Layer | Purpose | Config | ~Tokens |
|-------|---------|--------|---------|
| LLM Message Array | Direct conversation turns | 12 messages | 3-5K |
| Rolling Summary | Compressed older history | Every 100 msgs | ~2.5K |
| Semantic Search | Relevant distant memories | 10 matches | 1-2K |

---

## 8. Operation Engine

### 8.1 Lifecycle States

```
PENDING → STARTED → DELEGATING → GENERATING → COMPLETED
                                               ↓
                                            FAILED
```

### 8.2 Operation Kinds

- `code_generation` - Create new code
- `code_modification` - Edit existing code
- `code_review` - Analyze & critique
- `refactor` - Improve structure
- `debug` - Troubleshoot issues

### 8.3 Tool Calling Loop

```
Operation Context + User Prompt
    ↓
Build Prompt (system + memory + code + message history)
    ↓
LLM Call (with tools in context)
    ↓
Parse Tool Calls
    ↓
Route Each Tool (file ops, git, code analysis)
    ↓
Emit Events (tool execution, results)
    ↓
Next LLM Call (with tool results) or COMPLETED
```

---

## 9. WebSocket Protocol

### 9.1 Client → Server Messages

```json
// Chat message
{
  "type": "chat",
  "content": "User input",
  "project_id": "proj-123",
  "session_id": "sess-456",
  "system_access_mode": "project",
  "metadata": { "file_path": "src/main.rs" }
}

// Command (slash commands)
{
  "type": "command",
  "command": "fork",
  "args": { "target_session": "parent-id" }
}

// Project operations
{
  "type": "project_command",
  "method": "create",
  "params": { "name": "my-project" }
}

// Git operations
{
  "type": "git_command",
  "method": "git.diff",
  "params": { "project_id": "proj-123", "target": "uncommitted" }
}

// Sudo approval
{
  "type": "sudo_approval",
  "approval_id": "req-789",
  "approved": true
}
```

### 9.2 Server → Client Messages

```json
// Stream token
{
  "type": "stream_token",
  "operation_id": "op-123",
  "token": "class",
  "progress": 0.45
}

// Chat complete
{
  "type": "chat_complete",
  "operation_id": "op-123",
  "tokens_used": 1234,
  "cost_usd": 0.045,
  "cache_hit": false
}

// Operation event
{
  "type": "operation_event",
  "event_type": "tool_executed",
  "data": { "tool_name": "read_file", "success": true }
}

// Sudo approval request
{
  "type": "sudo_approval_request",
  "request_id": "req-789",
  "command": "apt install nginx",
  "reason": "Install web server"
}
```

---

## 10. Tools & Capabilities

### 10.1 Code Generation Tools
- `generate_code` - Create new file
- `refactor_code` - Improve existing code
- `debug_code` - Diagnose & fix issues

### 10.2 Code Intelligence Tools
- `find_function` - Locate functions by name/pattern
- `find_class_or_struct` - Find type definitions
- `search_code_semantic` - Semantic code search
- `find_imports` - Locate dependencies
- `analyze_dependencies` - Understand relationships
- `get_complexity_hotspots` - Find complex areas
- `get_quality_issues` - Code quality analysis
- `get_file_symbols` - List all symbols
- `find_tests_for_code` - Locate related tests
- `find_callers` - Find who calls a function
- `get_element_definition` - Get implementation

### 10.3 File Operation Tools
- `read_project_file` - Read code (max 500 lines)
- `write_project_file` - Create/modify files
- `edit_project_file` - Edit specific sections
- `search_codebase` - Search across files
- `list_project_files` - Browse directory
- `get_file_summary` - High-level overview

### 10.4 Git Analysis Tools
- `git_log` - Commit history
- `get_file_cochanges` - Files changed together
- `get_author_expertise` - Who's the expert?
- `blame_file` - Line-by-line history
- `get_historical_fixes` - How was this fixed before?

### 10.5 External Tools
- `web_search` - Search the internet (DuckDuckGo)
- `fetch_url` - Retrieve web content
- `run_command` - Execute shell commands
- `run_tests` - Execute test scenarios

---

## 11. Configuration System

### 11.1 LLM Configuration
```env
OPENAI_API_KEY=sk-...
MODEL_ROUTER_ENABLED=true
MODEL_FAST=gpt-5.1-codex-mini
MODEL_VOICE=gpt-5.1
MODEL_CODE=gpt-5.1-codex-max
MODEL_AGENTIC=gpt-5.1-codex-max
ROUTE_CODE_TOKEN_THRESHOLD=50000
ROUTE_CODE_FILE_COUNT=3
```

### 11.2 Budget Management
```env
BUDGET_DAILY_LIMIT_USD=5.0
BUDGET_MONTHLY_LIMIT_USD=150.0
```

### 11.3 Memory System
```env
MIRA_CONTEXT_RECENT_MESSAGES=30
MIRA_CONTEXT_SEMANTIC_MATCHES=10
MIRA_LLM_MESSAGE_HISTORY_LIMIT=12
MEM_SALIENCE_MIN_FOR_EMBED=0.5
MIRA_SUMMARY_ROLLING_ENABLED=true
```

### 11.4 Caching
```env
CACHE_ENABLED=true
CACHE_TTL_SECONDS=86400
```

### 11.5 Database & Storage
```env
DATABASE_URL=sqlite:./data/mira.db
QDRANT_URL=http://localhost:6334
```

### 11.6 Server
```env
MIRA_HOST=0.0.0.0
MIRA_PORT=3001
```

---

## 12. Session Architecture

### 12.1 Dual-Session Model

**Voice Sessions (Eternal):**
- Live across conversations
- User personality continuity
- Use GPT-5.1 Voice tier
- Parent session for Codex spawning

**Codex Sessions (Discrete):**
- Task-scoped for code work
- Use GPT-5.1-Codex-Max
- Spawned from Voice sessions
- Summarize back to Voice on completion

### 12.2 Session Tables

```sql
-- Voice session (eternal)
INSERT INTO chat_sessions
  (id, user_id, session_type, status)
VALUES ('voice-123', 'user-1', 'voice', 'active');

-- Codex session (discrete)
INSERT INTO chat_sessions
  (id, parent_session_id, session_type, codex_status)
VALUES ('codex-456', 'voice-123', 'codex', 'active');
```

---

## 13. CLI Architecture

### 13.1 Entry Point
`src/bin/mira.rs` - CLI binary

### 13.2 Key Components

| Module | Purpose |
|--------|---------|
| `repl.rs` | Interactive REPL loop |
| `ws_client.rs` | WebSocket connection |
| `session/` | Session management |
| `project/` | Project detection |
| `commands/` | Slash command loading |
| `display/` | Terminal output |

### 13.3 Built-in Commands

| Command | Description |
|---------|-------------|
| `/resume [name\|id]` | Resume session |
| `/resume --last` | Resume most recent |
| `/review` | Review uncommitted changes |
| `/review --branch <base>` | Review against branch |
| `/rename <name>` | Rename session |
| `/agents` | List background agents |
| `/agents cancel <id>` | Cancel agent |
| `/search <query>` | Web search |
| `/status` | Show session status |

---

## 14. Git Intelligence

### 14.1 Components (`src/git/intelligence/`)

| Module | Purpose |
|--------|---------|
| `commits.rs` | Index and track commits |
| `cochange.rs` | Detect files changed together |
| `blame.rs` | Line-by-line history |
| `expertise.rs` | Score author expertise |
| `fixes.rs` | Learn from historical fixes |

### 14.2 Co-Change Detection

Tracks files frequently modified together to suggest related files during edits.

### 14.3 Expertise Scoring

Scores author knowledge per file pattern based on:
- Number of commits
- Lines changed
- Recency of changes

---

## 15. Build System Integration

### 15.1 Components (`src/build/`)

| Module | Purpose |
|--------|---------|
| `runner.rs` | Execute build commands |
| `parser.rs` | Parse error output |
| `tracker.rs` | Store build runs |
| `resolver.rs` | Learn from fixes |

### 15.2 Error Learning

- Parse build errors (multi-language)
- Track how errors were resolved
- Suggest fixes for similar errors

---

## 16. Testing Infrastructure

### 16.1 Scenario Tests

```bash
cargo run --bin mira-test -- run ./scenarios/
cargo run --bin mira-test -- run ./scenarios/ --mock
cargo run --bin mira-test -- run ./scenarios/ --output json
```

### 16.2 Scenario Format

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
    prompt: "User message"
    assertions:
      - type: completed_successfully
      - type: tool_executed
        tool_name: list_project_files
```

### 16.3 Unit/Integration Tests

Located in `backend/tests/`:
- 17 test suites
- 160+ individual tests
- Real LLM tests (require API key)

---

## 17. Dependencies & External Systems

### 17.1 Runtime Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| Axum | 0.8 | Web framework |
| Tokio | 1.38 | Async runtime |
| SQLx | 0.8 | Database driver |
| Qdrant | 1.15 | Vector database client |
| Reqwest | 0.12 | HTTP client |
| Git2 | 0.20 | Git operations |
| SWC | 27 | JS/TS parser |

### 17.2 External Services

| Service | Purpose |
|---------|---------|
| OpenAI GPT-5.1 Family | LLM operations |
| OpenAI Embeddings | text-embedding-3-large (3072D) |
| Qdrant | Vector database (port 6334) |
| SQLite | Local database |

### 17.3 File System Locations

| Path | Content |
|------|---------|
| `backend/data/mira.db` | SQLite database |
| `backend/repos/` | Cloned repositories |
| `backend/storage/documents/` | Uploaded documents |
| `backend/qdrant_storage/` | Qdrant data |

---

## Appendix: Quick Reference

### Running the Backend

```bash
# Development
cargo run

# Production
cargo build --release
./target/release/mira-backend

# With debug logging
RUST_LOG=debug cargo run
```

### Database Operations

```bash
# Run migrations
DATABASE_URL="sqlite://data/mira.db" sqlx migrate run

# Reset database
./scripts/db-reset.sh
```

### Service Management

```bash
mira-ctl start all
mira-ctl status
mira-ctl logs backend -f
mira-ctl rebuild
```
