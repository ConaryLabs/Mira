# Mira Backend Technical Whitepaper

This document provides a comprehensive technical reference for the Mira backend architecture, designed to help LLMs understand how the system works.

## System Overview

Mira is an AI coding assistant with a Rust backend that uses:
- **GPT 5.1** for all LLM operations with variable reasoning effort
- **SQLite** for structured data storage (50+ tables)
- **Qdrant** for vector embeddings (3 collections)
- **WebSocket** for real-time client communication

## Architecture Layers

```
┌─────────────────────────────────────────────────────────┐
│                    WebSocket Layer                       │
│              (src/api/ws/)                              │
│    Handles client connections, message routing          │
└─────────────────────────────────────────────────────────┘
                           │
┌─────────────────────────────────────────────────────────┐
│                   Operation Engine                       │
│              (src/operations/engine/)                   │
│    Orchestrates complex workflows, tool execution       │
└─────────────────────────────────────────────────────────┘
                           │
        ┌──────────────────┼──────────────────┐
        │                  │                  │
┌───────v───────┐  ┌───────v───────┐  ┌───────v───────┐
│  GPT 5.1      │  │  Memory       │  │  Git          │
│  Provider     │  │  Service      │  │  Intelligence │
│  (src/llm/)   │  │  (src/memory/)│  │  (src/git/)   │
└───────────────┘  └───────┬───────┘  └───────────────┘
                           │
              ┌────────────┼────────────┐
              │            │            │
        ┌─────v─────┐ ┌────v────┐ ┌─────v─────┐
        │  SQLite   │ │ Qdrant  │ │ Code      │
        │  Storage  │ │ Vectors │ │ Intel     │
        └───────────┘ └─────────┘ └───────────┘
```

## Core Components

### 1. GPT 5.1 Provider (`src/llm/provider/gpt5.rs`)

The LLM provider handles all GPT 5.1 API interactions.

**Key Features:**
- Variable reasoning effort: `Minimum`, `Medium`, `High`
- Tool calling via OpenAI Chat Completions API format
- Streaming support via Server-Sent Events (SSE)
- Response caching integration

**Usage:**
```rust
let provider = Gpt5Provider::new(api_key, model, ReasoningEffort::Medium)?;
let response = provider.complete(messages, reasoning_effort).await?;
let stream = provider.complete_stream(messages, reasoning_effort).await?;
```

**Tool Format:**
Tools must be in OpenAI-compatible format:
```json
{
  "type": "function",
  "function": {
    "name": "read_file",
    "description": "Read contents of a file",
    "parameters": {
      "type": "object",
      "properties": {
        "path": {"type": "string", "description": "File path"}
      },
      "required": ["path"]
    }
  }
}
```

### 2. Operation Engine (`src/operations/engine/`)

The Operation Engine orchestrates complex multi-step workflows.

**Components:**
- `orchestration.rs` - Main execution loop
- `lifecycle.rs` - Operation state management
- `context.rs` - Context building for LLM calls
- `tool_router.rs` - Routes tool calls to handlers
- `skills.rs` - Skill definitions and routing

**Operation Lifecycle:**
```
PENDING → STARTED → DELEGATING → GENERATING → COMPLETED
                                               ↓
                                            FAILED
```

**Tool Categories:**
- File operations: `read_file`, `write_file`, `list_files`, etc.
- Git operations: `git_status`, `git_commit`, `git_diff`, etc.
- Code analysis: `find_functions`, `find_classes`, `semantic_search`, etc.
- External: `web_search`, `fetch_url`, `execute_command`

**Tool Execution Flow:**
1. LLM returns tool_calls in response
2. ToolRouter routes each call to appropriate handler
3. Handler executes and returns result
4. Results sent back to LLM for continuation

### 3. Memory Service (`src/memory/`)

The Memory Service coordinates all memory operations.

**Storage Backends:**
- **SQLite** (`src/memory/storage/sqlite/`): Structured data
- **Qdrant** (`src/memory/storage/qdrant/`): Vector embeddings

**Qdrant Collections:**
- `code`: Semantic nodes, code elements, patterns
- `conversation`: Messages, summaries, user context
- `git`: Commits, co-change patterns, fixes

**Key Tables (SQLite):**
- `memory_entries`: Conversation history
- `message_analysis`: Extracted mood, intent, topics, salience
- `rolling_summaries`: Compressed context (10-msg and 100-msg windows)
- `memory_facts`: Key-value facts about user
- `user_profile`: Coding preferences, tech stack

**Recall Engine:**
The recall engine assembles context for each LLM call:
1. Recent messages (chronological)
2. Semantic search results (from Qdrant)
3. Rolling summaries (compressed history)
4. Code intelligence (if applicable)
5. Git intelligence (if applicable)

### 4. Code Intelligence (`src/memory/features/code_intelligence/`)

Code intelligence provides semantic understanding of code.

**Modules:**
- `semantic.rs`: Semantic graph (nodes, edges, concepts)
- `call_graph.rs`: Function call relationships
- `patterns.rs`: Design pattern detection
- `clustering.rs`: Domain-based code grouping
- `cache.rs`: Analysis result caching

**Semantic Graph:**
```
semantic_nodes: id, element_id, purpose, concepts, domain_labels
semantic_edges: source_id, target_id, relationship_type, weight
concept_index: concept, node_id (enables concept-based search)
```

**Relationship Types:**
- `Uses`, `Implements`, `Extends`, `Contains`, `Calls`
- `ConceptSimilar`, `DomainSame`, `CoChange`

**Call Graph:**
- Stores caller-callee relationships
- Supports impact analysis (what breaks if X changes)
- Path finding between functions
- Entry point and leaf detection

**Pattern Detection:**
Detects: Factory, Builder, Repository, Observer, Singleton, Strategy, Decorator, Adapter, Facade, Command, Iterator

### 5. Git Intelligence (`src/git/intelligence/`)

Git intelligence provides deep understanding of git history.

**Modules:**
- `commits.rs`: Commit tracking with file changes
- `cochange.rs`: Co-change pattern detection
- `blame.rs`: Line-level blame annotations
- `expertise.rs`: Author expertise scoring
- `fixes.rs`: Historical fix matching

**Co-change Detection:**
Files that frequently change together get tracked:
- Confidence score: `cochange_count / (changes_a + changes_b - cochange_count)` (Jaccard)
- Suggests related files when editing

**Author Expertise:**
Scoring formula: `40% commits + 30% lines_changed + 30% recency`
- Recency uses 365-day exponential decay
- Tracks expertise per file and per domain

**Historical Fixes:**
- Normalizes error patterns (removes paths, numbers, quoted strings)
- Links errors to fix commits
- Suggests past fixes for similar errors

### 6. Budget & Cache (`src/budget/`, `src/cache/`)

**Budget Tracking:**
- Records every LLM API call with cost
- Enforces daily/monthly spending limits
- Tracks token usage and cache hit rate

**LLM Cache:**
- SHA-256 key generation from request components
- Cache key = hash(messages + tools + system + model + reasoning_effort)
- TTL-based expiration (default 24 hours)
- LRU eviction when cache grows large
- Target 80%+ hit rate

## Database Schema Overview

### Foundation Tables (001)
- `users`, `sessions`, `user_profile`
- `projects`, `git_repo_attachments`, `repository_files`
- `memory_entries`, `message_analysis`, `rolling_summaries`
- `memory_facts`, `learned_patterns`, `message_embeddings`

### Code Intelligence Tables (002)
- `code_elements`: AST-parsed code symbols
- `semantic_nodes`, `semantic_edges`, `concept_index`
- `call_graph`, `external_dependencies`
- `design_patterns`, `pattern_validation_cache`
- `domain_clusters`, `code_quality_issues`

### Git Intelligence Tables (003)
- `git_commits`: Full commit history
- `file_cochange_patterns`: Co-change tracking
- `blame_annotations`: Line-level blame
- `author_expertise`: Per-file expertise scores
- `historical_fixes`: Error-to-fix mappings

### Operations Tables (004)
- `operations`: Workflow instances
- `operation_events`: Real-time event log
- `operation_tasks`: Task breakdown
- `artifacts`: Generated code artifacts

### Tool Synthesis Tables (006)
- `tool_patterns`: Detected automation patterns
- `synthesized_tools`: Generated tools
- `tool_executions`: Execution history
- `tool_effectiveness`: Success metrics

### Budget & Cache Tables (008)
- `budget_tracking`: Per-request cost records
- `budget_summary`: Daily/monthly aggregates
- `llm_cache`: Response cache
- `reasoning_patterns`, `reasoning_steps`, `pattern_usage`

## WebSocket Protocol

### Message Types (Client → Server)
```json
{"type": "chat", "content": "user message", "session_id": "..."}
{"type": "operation.cancel", "operation_id": "..."}
```

### Message Types (Server → Client)
```json
{"type": "operation.started", "operation_id": "..."}
{"type": "operation.chunk", "content": "partial response"}
{"type": "operation.tool_call", "tool": "read_file", "args": {...}}
{"type": "operation.tool_result", "result": "..."}
{"type": "operation.completed", "response": "full response"}
{"type": "operation.failed", "error": "..."}
```

### Event Flow
1. Client sends chat message
2. Server creates operation, sends `operation.started`
3. LLM generates response, server streams `operation.chunk`
4. If tools needed: `operation.tool_call` → execute → `operation.tool_result`
5. Repeat tool cycle until LLM done
6. Server sends `operation.completed`

## Configuration

### Environment Variables
```bash
# Server
MIRA_PORT=3001
MIRA_ENV=development

# Database
DATABASE_URL=sqlite://mira.db
QDRANT_URL=http://localhost:6334

# GPT 5.1
OPENAI_API_KEY=sk-...
GPT5_MODEL=gpt-5.1
GPT5_REASONING_DEFAULT=medium

# Embeddings
OPENAI_EMBEDDING_MODEL=text-embedding-3-large

# Budget
BUDGET_DAILY_LIMIT_USD=5.0
BUDGET_MONTHLY_LIMIT_USD=150.0

# Cache
CACHE_ENABLED=true
CACHE_TTL_SECONDS=86400
```

## Key Patterns

### Error Handling
- Use `anyhow::Result` for propagating errors
- Log errors with `tracing` before returning
- Return user-friendly error messages via WebSocket

### Async Patterns
- All I/O operations are async (SQLite, Qdrant, HTTP)
- Use `tokio::spawn` for background tasks
- Channels for inter-task communication

### Arc Sharing
- Services wrapped in `Arc` for thread-safe sharing
- `AppState` holds all service instances
- Clone `Arc` references, not the services themselves

### Database Queries
- Use `sqlx::query` with runtime queries (not compile-time)
- Handle `Option<T>` for nullable columns
- Use transactions for multi-statement operations

## Testing

### Test Structure
- Integration tests in `backend/tests/`
- 17 test suites, 127+ tests
- Tests use in-memory SQLite and fake API keys

### Running Tests
```bash
# All tests
DATABASE_URL="sqlite://mira.db" cargo test

# Specific suite
DATABASE_URL="sqlite://mira.db" cargo test --test git_intelligence_test

# With output
DATABASE_URL="sqlite://mira.db" cargo test -- --nocapture
```

### Test Helpers
- `tests/common/` - Shared test utilities
- In-memory database creation
- Fake provider construction

## Performance Considerations

### Caching Strategy
- LLM responses cached with SHA-256 keys
- Semantic analysis cached per symbol
- Pattern validation cached per detection
- Blame annotations cached with content hash

### Database Optimization
- SQLite WAL mode for concurrency
- Indexes on frequently queried columns
- Batch inserts for bulk operations

### Memory Management
- Streaming for large LLM responses
- Pagination for Qdrant scrolling
- Rolling summaries compress old context

## Information Flow & Context Pipeline

This section documents how information flows through the system - from storage to LLM context injection.

### High-Level Flow

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              USER MESSAGE                                    │
└─────────────────────────────────────────────────────────────────────────────┘
                                      │
                                      ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                           OPERATION ENGINE                                   │
│                    (src/operations/engine/orchestration.rs)                  │
└─────────────────────────────────────────────────────────────────────────────┘
                                      │
            ┌─────────────────────────┼─────────────────────────┐
            ▼                         ▼                         ▼
┌───────────────────┐    ┌───────────────────────┐    ┌────────────────────┐
│   RECALL ENGINE   │    │    CONTEXT ORACLE     │    │   CONTEXT BUILDER  │
│  (Memory Context) │    │  (Code Intelligence)  │    │  (Prompt Assembly) │
│  recall_engine/   │    │   context_oracle/     │    │ engine/context.rs  │
└─────────┬─────────┘    └───────────┬───────────┘    └─────────┬──────────┘
          │                          │                          │
          │                          ▼                          │
          │              ┌───────────────────────┐              │
          │              │   GATHERED CONTEXT    │              │
          │              │  • Guidelines         │              │
          │              │  • Code Search        │              │
          │              │  • Semantic Concepts  │              │
          │              │  • Call Graph         │              │
          │              │  • Co-change          │              │
          │              │  • Historical Fixes   │              │
          │              │  • Design Patterns    │              │
          │              │  • Reasoning Patterns │              │
          │              │  • Build Errors       │              │
          │              │  • Error Resolutions  │              │
          │              │  • Expertise          │              │
          │              └───────────┬───────────┘              │
          │                          │                          │
          └──────────────────────────┼──────────────────────────┘
                                     ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                     UNIFIED PROMPT BUILDER                                   │
│                      (src/prompt/builders.rs)                               │
│                                                                             │
│  Assembles: Persona + Memory + Code Intelligence + Tools + File Context     │
└─────────────────────────────────────────────────────────────────────────────┘
                                     │
                                     ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                            GPT 5.1 API CALL                                 │
│                      System Prompt + User Message                           │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Context Oracle: Intelligence Hub

The Context Oracle (`src/context_oracle/`) is the central hub for gathering intelligence from all subsystems.

**Architecture:**
```rust
pub struct ContextOracle {
    pool: Arc<SqlitePool>,
    code_intelligence: Option<Arc<CodeIntelligenceService>>,
    semantic_graph: Option<Arc<SemanticGraphService>>,
    guidelines_service: Option<Arc<ProjectGuidelinesService>>,
    cochange_service: Option<Arc<CochangeService>>,
    expertise_service: Option<Arc<ExpertiseService>>,
    fix_service: Option<Arc<FixService>>,
    build_tracker: Option<Arc<BuildTracker>>,
    error_resolver: Option<Arc<ErrorResolver>>,
    pattern_storage: Option<Arc<PatternStorage>>,
    pattern_matcher: Option<Arc<PatternMatcher>>,
}
```

**Gathering Pipeline:**

| Source | Method | Data Retrieved | SQLite Table(s) | Qdrant Collection |
|--------|--------|----------------|-----------------|-------------------|
| Guidelines | `gather_guidelines()` | Project guidelines content | `project_guidelines` | - |
| Code Search | `gather_code_context()` | Semantically relevant code | `code_elements` | `code` |
| Semantic Concepts | `gather_semantic_concepts()` | Related symbols by concept | `semantic_nodes`, `concept_index` | - |
| Call Graph | `gather_call_graph_context()` | Callers/callees of functions | `call_graph` | - |
| Co-change | `gather_cochange_suggestions()` | Files that change together | `file_cochange_patterns` | - |
| Historical Fixes | `gather_historical_fixes()` | Past fixes for similar errors | `historical_fixes` | - |
| Design Patterns | `gather_design_patterns()` | Detected patterns (Factory, etc.) | `design_patterns` | - |
| Reasoning Patterns | `gather_reasoning_patterns()` | Matched reasoning strategies | `reasoning_patterns` | - |
| Build Errors | `gather_build_errors()` | Recent compilation errors | `build_errors` | - |
| Error Resolutions | `gather_error_resolutions()` | How past errors were fixed | `error_resolutions` | - |
| Expertise | `gather_expertise()` | Expert authors for file/domain | `author_expertise` | - |

### Context Configuration

Context gathering is budget-aware and configurable:

```rust
pub struct ContextConfig {
    pub include_code_search: bool,         // Semantic code search
    pub include_semantic_concepts: bool,   // Concept-based code understanding
    pub include_guidelines: bool,          // Project guidelines
    pub include_call_graph: bool,          // Function relationships
    pub include_cochange: bool,            // Co-change suggestions
    pub include_historical_fixes: bool,    // Past error fixes
    pub include_patterns: bool,            // Design patterns
    pub include_reasoning_patterns: bool,  // Reasoning strategies
    pub include_build_errors: bool,        // Recent build errors
    pub include_error_resolutions: bool,   // How errors were resolved
    pub include_expertise: bool,           // Author expertise
    pub max_context_tokens: usize,         // Token budget limit
    pub max_code_results: usize,           // Code search limit
    pub max_cochange_suggestions: usize,   // Co-change limit
    pub max_historical_fixes: usize,       // Fix history limit
}
```

**Budget-Aware Presets:**

| Preset | Budget Usage | Token Limit | Features Enabled |
|--------|-------------|-------------|------------------|
| `full()` | <40% | 16,000 | All features, max results |
| `default()` | 40-80% | 8,000 | All features, standard limits |
| `minimal()` | >80% | 4,000 | Code search + guidelines only |
| `for_error()` | Any | 12,000 | Error-focused (fixes, resolutions, build errors) |

### Recall Engine: Memory Context

The Recall Engine (`src/memory/features/recall_engine/`) provides conversation memory context.

**Components:**
- `RecentSearch`: Chronological message retrieval from SQLite
- `SemanticSearch`: Vector similarity search from Qdrant
- `HybridSearch`: Combined recent + semantic with scoring
- `MultiHeadSearch`: Multi-embedding search (emotional, factual, code)

**RecallContext Structure:**
```rust
pub struct RecallContext {
    pub recent: Vec<MemoryEntry>,           // Recent messages
    pub semantic: Vec<MemoryEntry>,         // Semantically similar
    pub rolling_summary: Option<String>,    // Last 100 messages compressed
    pub session_summary: Option<String>,    // Full session summary
    pub code_intelligence: Option<GatheredContext>,  // From Context Oracle
}
```

### Prompt Assembly

The UnifiedPromptBuilder (`src/prompt/builders.rs`) assembles the final system prompt:

**Assembly Order:**
1. **Persona** - Core personality from `src/persona/default.rs`
2. **Project Context** - Active project name and metadata
3. **Memory Context** - Summaries + recent + semantic memories
4. **Code Intelligence** - From semantic search results
5. **Repository Structure** - File tree (directories and key files)
6. **Tool Context** - Available tools and usage hints
7. **File Context** - Currently viewed file content

**Enriched Context (for tool execution):**
The ContextBuilder (`src/operations/engine/context.rs`) builds enriched context for tool-assisted operations:

```
=== TASK CONTEXT ===
[LLM's context from tool call]

=== PROJECT STRUCTURE ===
[Directory tree, max depth 3]

=== RELEVANT CODE CONTEXT ===
[Top 5 semantic search results with file paths]

=== CODEBASE INTELLIGENCE ===
[Context Oracle output - formatted sections for each source]

=== USER PREFERENCES & CODING STYLE ===
[Rolling summary + relevant semantic memories]
```

### GatheredContext Formatting

The Context Oracle formats its output via `GatheredContext::format_for_prompt()`:

```markdown
## Project Guidelines
[Guidelines content with file path prefixes]

## Relevant Code
**function** `func_name` in `src/file.rs`
```code```

## Related Concepts
- **concept** (domain): purpose
  Related: symbol1, symbol2

## Call Graph
**Callers**: func1, func2
**Callees**: func3, func4

## Related Files (Often Changed Together)
- `src/related.rs` (85% confidence): reason

## Similar Past Fixes
- **abc1234** (90% similar): fix description

## Detected Patterns
- **FactoryPattern** (Factory): description

## Suggested Approach
**PatternName** (85% match)
description
Steps:
1. Step one
2. Step two

## Recent Build Errors
- **category**: error message

## Past Error Resolutions
- **fix_type**: Fixed by commit in files: file1, file2

## Domain Experts
- **author@email** (90% expertise): areas
```

### Data Storage Summary

**Where Data is Written:**

| Data Type | Service | SQLite Table | Qdrant Collection |
|-----------|---------|--------------|-------------------|
| Messages | MemoryService | `memory_entries` | `conversation` |
| Message Analysis | MessagePipeline | `message_analysis` | - |
| Rolling Summaries | SummarizationService | `rolling_summaries` | - |
| Memory Facts | FactsService | `memory_facts` | - |
| Code Elements | CodeIntelligenceService | `code_elements` | `code` |
| Semantic Nodes | SemanticGraphService | `semantic_nodes` | - |
| Call Graph | CallGraphService | `call_graph` | - |
| Design Patterns | PatternDetector | `design_patterns` | - |
| Git Commits | GitStore | `git_commits` | `git` |
| Co-change Patterns | CochangeService | `file_cochange_patterns` | - |
| Author Expertise | ExpertiseService | `author_expertise` | - |
| Historical Fixes | FixService | `historical_fixes` | - |
| Build Errors | BuildTracker | `build_errors` | - |
| Error Resolutions | ErrorResolver | `error_resolutions` | - |
| Project Guidelines | ProjectGuidelinesService | `project_guidelines` | - |
| Reasoning Patterns | PatternStorage | `reasoning_patterns` | - |
| Budget Tracking | BudgetTracker | `budget_tracking` | - |
| LLM Cache | LlmCache | `llm_cache` | - |

**Where Data is Read (for LLM Context):**

| Context Source | Read By | Injected Via |
|----------------|---------|--------------|
| Recent Messages | RecallEngine | `add_memory_context()` |
| Semantic Memories | RecallEngine | `add_memory_context()` |
| Rolling Summary | RecallEngine | `add_memory_context()` |
| Session Summary | RecallEngine | `add_memory_context()` |
| Code Search Results | ContextOracle | `build_enriched_context_with_oracle()` |
| Semantic Concepts | ContextOracle | `GatheredContext::format_for_prompt()` |
| Call Graph | ContextOracle | `GatheredContext::format_for_prompt()` |
| Co-change | ContextOracle | `GatheredContext::format_for_prompt()` |
| Historical Fixes | ContextOracle | `GatheredContext::format_for_prompt()` |
| Design Patterns | ContextOracle | `GatheredContext::format_for_prompt()` |
| Reasoning Patterns | ContextOracle | `GatheredContext::format_for_prompt()` |
| Build Errors | ContextOracle | `GatheredContext::format_for_prompt()` |
| Error Resolutions | ContextOracle | `GatheredContext::format_for_prompt()` |
| Expertise | ContextOracle | `GatheredContext::format_for_prompt()` |
| Guidelines | ContextOracle | `GatheredContext::format_for_prompt()` |
| File Tree | ContextLoader | `add_repository_structure()` |
| Tools | delegation_tools | `add_tool_context()` |

### Integration Points

**AppState Wiring (`src/state.rs`):**
```rust
let context_oracle = Arc::new(
    ContextOracle::new(Arc::new(pool.clone()))
        .with_code_intelligence(code_intelligence.clone())
        .with_semantic_graph(semantic_graph.clone())
        .with_guidelines(guidelines_service.clone())
        .with_cochange(cochange_service.clone())
        .with_expertise(expertise_service.clone())
        .with_fix_service(fix_service.clone())
        .with_build_tracker(build_tracker.clone())
        .with_error_resolver(error_resolver.clone())
        .with_pattern_storage(pattern_storage.clone())
        .with_pattern_matcher(pattern_matcher.clone()),
);
```

**MemoryService with Oracle:**
```rust
let memory_service = Arc::new(MemoryService::with_oracle(
    sqlite_store.clone(),
    multi_store.clone(),
    gpt5_provider.clone(),
    embedding_client.clone(),
    Some(context_oracle.clone()),
));
```

**OperationEngine with Oracle:**
```rust
let operation_engine = Arc::new(OperationEngine::new(
    // ... other params
    Some(context_oracle.clone()),  // Context Oracle for unified intelligence
));
```

## Security

### API Key Management
- Keys stored in environment variables
- Never logged or included in responses
- Validated on startup

### Tool Execution
- File operations restricted by default
- `unrestricted: true` flag for system-wide access
- Command execution sandboxed with timeout

### Input Validation
- Session IDs validated
- File paths checked for traversal attacks
- User input sanitized before storage
