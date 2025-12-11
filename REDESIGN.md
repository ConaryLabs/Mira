# Mira Redesign: Claude Code Power Suit

## Vision

Mira becomes a **memory and intelligence layer** for Claude Code, not a competing assistant. Claude Code handles all LLM orchestration, conversation, and tool execution. Mira provides superpowers Claude Code lacks:

- **Persistent memory** across sessions
- **Semantic search** over past work
- **Code intelligence** (relationships, patterns, call graphs)
- **Git intelligence** (co-change, expertise, historical fixes)
- **Build error learning** (what fixed what)

## Architecture

```
┌────────────────────────────────────────────────────────────────┐
│                        Claude Code                              │
│                   (drives everything)                           │
└───────────────────────────┬────────────────────────────────────┘
                            │ MCP (stdio)
                            ▼
┌────────────────────────────────────────────────────────────────┐
│                      mira (power suit)                          │
│                                                                 │
│   ┌──────────────────────────────────────────────────────┐     │
│   │                    MCP Server                         │     │
│   │  Tools exposed to Claude Code via JSON-RPC            │     │
│   └──────────────────────────────────────────────────────┘     │
│                              │                                  │
│   ┌──────────┬───────────┬───┴────┬─────────────┬──────────┐   │
│   │  Memory  │   Code    │  Git   │    Build    │ Project  │   │
│   │  Store   │   Intel   │  Intel │   Learning  │ Context  │   │
│   └────┬─────┴─────┬─────┴────┬───┴──────┬──────┴────┬─────┘   │
│        │           │          │          │           │          │
│   ┌────┴───┐  ┌────┴───┐  ┌───┴───┐  ┌───┴───┐  ┌───┴───┐     │
│   │ SQLite │  │ Qdrant │  │  Git  │  │ Build │  │ Watch │     │
│   │        │  │        │  │ Repo  │  │ Cache │  │  FS   │     │
│   └────────┘  └────────┘  └───────┘  └───────┘  └───────┘     │
└────────────────────────────────────────────────────────────────┘
```

## MCP Tool Categories

### 1. Memory (Persistent Knowledge)

| Tool | Description |
|------|-------------|
| `remember` | Store a fact, decision, or preference |
| `recall` | Semantic search across all memories |
| `get_memories` | Get recent memories for context |
| `forget` | Remove a memory |

**Use cases:**
- "Remember that we use snake_case in this project"
- "What did we decide about the auth approach?"
- "How did I fix that postgres connection issue?"

### 2. Code Intelligence

| Tool | Description |
|------|-------------|
| `analyze_file` | Get full analysis: dependencies, patterns, relationships |
| `find_similar_code` | Semantic search for similar code |
| `get_call_graph` | Function/method call relationships |
| `get_related_files` | Files that are semantically or structurally related |
| `get_symbols` | Get symbols (functions, classes, etc.) in a file |

**Use cases:**
- "What other files might I need to change?"
- "Find code that does something similar to this"
- "What calls this function?"

### 3. Git Intelligence

| Tool | Description |
|------|-------------|
| `get_cochange_files` | Files that usually change together |
| `get_file_experts` | Authors with most expertise on file/area |
| `find_similar_fixes` | Past fixes for similar issues |
| `get_change_risk` | Risk assessment for proposed changes |

**Use cases:**
- "Who should review changes to auth?"
- "Has this type of bug been fixed before?"
- "How risky is changing this file?"

### 4. Build Intelligence

| Tool | Description |
|------|-------------|
| `report_error` | Track a build/test error |
| `suggest_fix` | Get fix suggestions from history |
| `get_error_history` | Past errors in file/area |

**Use cases:**
- "I'm getting this compile error, have we seen it?"
- "What usually fixes this type of test failure?"

### 5. Project Context

| Tool | Description |
|------|-------------|
| `get_project_context` | High-level project summary |
| `get_file_context` | Everything relevant about a file |
| `get_guidelines` | Project-specific conventions/rules |
| `add_guideline` | Add a project convention |

**Use cases:**
- Automatic context injection for new conversations
- "What are the conventions for this project?"

## What Gets Removed

### Delete Entirely

```
src/llm/                    # All LLM providers and orchestration
src/operations/             # Operation engine, tool execution
src/api/ws/chat/            # Chat handling, message routing
src/prompt/                 # Prompt building
src/cli/                    # Interactive CLI (repl, commands)
src/session/                # Session management for chat
src/bin/mira.rs             # CLI binary
src/bin/mira_test.rs        # Test harness (LLM-based)
src/main.rs                 # WebSocket server
frontend/                   # Entire React frontend
```

### Keep and Enhance

```
src/memory/                 # Core memory system
  storage/                  # SQLite + Qdrant storage
  features/
    code_intelligence/      # Semantic graph, call graph, patterns
    recall_engine/          # Semantic search
src/git/                    # Git integration
  intelligence/             # Co-change, expertise, fixes
  client/                   # Git operations
src/build/                  # Build error tracking
src/watcher/                # File system watching
src/cache/                  # Caching layer
src/relationship/           # User facts (becomes memory facts)
```

### Transform

```
src/bin/mira_mcp_server.rs  → src/main.rs (only binary)
src/api/                    → Remove WebSocket, keep minimal HTTP for health
```

## Database Schema Changes

### Keep (possibly rename tables)

- `memory_entries` → `memories` (semantic memories)
- `memory_facts` → `facts` (structured facts)
- `file_cochange_patterns` (git intelligence)
- `author_expertise` (git intelligence)
- `historical_fixes` (git intelligence)
- `code_elements` (code intelligence)
- `semantic_nodes` / `semantic_edges` (code intelligence)
- `call_graph` (code intelligence)
- `design_patterns` (code intelligence)
- `build_errors` / `error_resolutions` (build intelligence)
- `project_guidelines` (project context)

### Remove

- `chat_sessions` (no chat)
- `operations` / `operation_events` / `operation_tasks` (no operations)
- `artifacts` (no artifact generation)
- `rolling_summaries` (no conversation summarization)
- `message_analysis` / `message_embeddings` (no message pipeline)
- `sudo_*` tables (no command execution)
- `terminal_*` tables (no terminal)
- `sessions` (auth sessions - maybe keep for multi-user)
- `budget_*` (Claude Code handles its own budget)
- `llm_cache` (Claude Code's problem)
- `codex_session_links` / `task_sessions` (no dual session)

## Implementation Status (Completed)

### Phase 1: Strip Down - DONE
- [x] Remove frontend entirely
- [x] Remove LLM orchestration (`src/llm/` - now just embedding provider)
- [x] Remove operations engine (`src/operations/`)
- [x] Remove chat/WebSocket handling
- [x] Remove CLI
- [x] Clean up Cargo.toml (remove unused deps)

### Phase 2: MCP Server Core - DONE
- [x] Make MCP server the main binary
- [x] Implement memory tools (remember, recall, forget)
- [x] Implement basic code intelligence tools
- [x] Test with Claude Code

### Phase 3: Intelligence Integration - DONE
- [x] Wire up git intelligence to MCP
- [x] Wire up code intelligence to MCP
- [x] Implement build error tracking
- [x] Background indexer for code/git analysis

### Phase 4: Polish - DONE
- [x] Performance optimization
- [x] Documentation
- [x] CLAUDE.md instructions for using Mira

### Code Structure (Final)

```
backend/src/
├── main.rs           # MCP server (22 tools)
├── lib.rs            # Library exports
├── llm.rs            # Embedding provider (OpenAI)
├── memory/
│   ├── context.rs    # Context types for recall
│   ├── features/
│   │   ├── prompts.rs  # System prompts for LLM features
│   │   └── ...
│   └── ...
├── git/
│   ├── error.rs      # Git error types
│   └── ...
├── build/            # Build error tracking
├── watcher/          # File system watching
└── config/           # Configuration
```

## Configuration

### Claude Code MCP Config (`~/.claude/mcp.json`)

```json
{
  "mcpServers": {
    "mira": {
      "command": "mira",
      "args": ["--project", "${workspaceFolder}"],
      "env": {
        "MIRA_DB": "~/.mira/mira.db",
        "QDRANT_URL": "http://localhost:6334"
      }
    }
  }
}
```

### Mira Config (`~/.mira/config.toml`)

```toml
[database]
path = "~/.mira/mira.db"

[qdrant]
url = "http://localhost:6334"

[indexing]
# Background indexing settings
watch_paths = true
index_on_startup = true

[memory]
# How long to keep memories
retention_days = 365
```

## Success Criteria

1. **Claude Code can remember** - Facts persist across sessions
2. **Claude Code can search** - "How did I solve X?" works
3. **Claude Code knows code** - Understands relationships, patterns
4. **Claude Code learns from git** - Knows who to ask, what changes together
5. **Claude Code learns from errors** - Suggests fixes based on history
6. **Startup is fast** - < 100ms to respond to MCP requests
7. **Resource light** - Minimal CPU/memory when idle

## Open Questions

1. **Multi-project support** - How to handle memories across projects?
2. **Embedding model** - Use OpenAI (cost) or local model (complexity)?
3. **Qdrant dependency** - Required or optional with fallback?
4. **Installation** - Homebrew? Cargo install? Binary download?
5. **CLAUDE.md integration** - Auto-inject Mira tool hints?
