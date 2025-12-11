# CLAUDE.md

This file provides guidance to Claude Code when working with code in this repository.

## Project Overview

Mira is a **Power Suit for Claude Code** - a memory and intelligence layer that provides persistent storage, semantic search, and cross-session context through the Model Context Protocol (MCP).

**Key concept**: Claude Code drives all AI interactions. Mira provides storage and intelligence capabilities that Claude Code can't do natively:
- Persistent memory across sessions with semantic search
- Cross-session context (what did we work on before?)
- Task tracking that persists
- Code intelligence (symbols, call graphs)
- Git intelligence (cochange patterns, error fixes)

## Repository Structure

```
mira/
├── src/
│   ├── main.rs       # MCP server entry point (348 lines)
│   ├── lib.rs        # Library exports (4 lines)
│   └── tools/        # MCP tool implementations
│       ├── mod.rs        # Module exports
│       ├── types.rs      # Request types for all tools
│       ├── response.rs   # Response helpers (json_response, etc.)
│       ├── semantic.rs   # SemanticSearch (Qdrant + OpenAI)
│       ├── memory.rs     # remember, recall, forget
│       ├── sessions.rs   # store_session, search_sessions
│       ├── tasks.rs      # task management
│       ├── code_intel.rs # symbols, call graph, semantic search
│       ├── git_intel.rs  # commits, cochange, error fixes
│       ├── build_intel.rs# build tracking
│       ├── documents.rs  # document search
│       ├── workspace.rs  # activity/context tracking
│       ├── project.rs    # guidelines
│       └── analytics.rs  # list_tables, query
├── migrations/       # Database schema
├── data/             # SQLite database (runtime, gitignored)
├── Cargo.toml        # Rust dependencies
└── .sqlx/            # SQLx offline mode cache
```

## Development Commands

```bash
# Build
SQLX_OFFLINE=true cargo build --release

# Run (as MCP server with semantic search)
DATABASE_URL="sqlite://data/mira.db" \
QDRANT_URL="http://localhost:6334" \
OPENAI_API_KEY="sk-..." \
./target/release/mira

# Linting
SQLX_OFFLINE=true cargo clippy
cargo fmt
```

## Architecture

### MCP Server

Single binary MCP server communicating via stdio JSON-RPC:

```
Claude Code  <--MCP(stdio)-->  Mira Server
                                   |
                     +-------------+-------------+
                     |             |             |
                  SQLite        Qdrant      Direct SQL
                 (facts,       (vectors)    (all queries)
                 sessions,     semantic
                 tasks)        search)
```

### Semantic Search

When Qdrant and OpenAI are configured:
- **Model**: text-embedding-3-large (3072 dimensions)
- **Collections**: mira_code, mira_conversation, mira_docs
- **Fallback**: Text search when Qdrant/OpenAI unavailable

### 36 MCP Tools

**Memory** (semantic search):
- `remember` - Store facts, decisions, preferences
- `recall` - Search memories by meaning
- `forget` - Delete a memory

**Cross-Session Context**:
- `store_session` - Store session summary
- `search_sessions` - Find past sessions
- `store_decision` - Record decisions

**Tasks**:
- `create_task`, `list_tasks`, `get_task`, `update_task`, `complete_task`, `delete_task`

**Code Intelligence**:
- `get_symbols` - File symbols
- `get_call_graph` - Call relationships
- `get_related_files` - Related files
- `semantic_code_search` - Natural language code search

**Git Intelligence**:
- `get_recent_commits`, `search_commits`
- `find_cochange_patterns` - Files that change together
- `find_similar_fixes` - Past error fixes
- `record_error_fix` - Record a fix

**Build Intelligence**:
- `record_build`, `record_build_error`, `get_build_errors`, `resolve_error`

**Documents**:
- `list_documents`, `search_documents`, `get_document`

**Workspace**:
- `record_activity`, `get_recent_activity`, `set_context`, `get_context`

**Project**:
- `get_guidelines`, `add_guideline`

**Analytics**:
- `list_tables`, `query`

## Configuration

### Claude Code MCP Config

Add to `~/.claude/mcp.json`:

```json
{
  "mcpServers": {
    "mira": {
      "command": "/path/to/mira/target/release/mira",
      "env": {
        "DATABASE_URL": "sqlite:///path/to/mira/data/mira.db",
        "QDRANT_URL": "http://localhost:6334",
        "OPENAI_API_KEY": "sk-..."
      }
    }
  }
}
```

### Environment Variables

| Variable | Description | Required |
|----------|-------------|----------|
| `DATABASE_URL` | SQLite connection | Yes |
| `QDRANT_URL` | Qdrant gRPC endpoint | No (semantic search disabled) |
| `OPENAI_API_KEY` | OpenAI embeddings | No (semantic search disabled) |
| `RUST_LOG` | Log level | No (default: info) |

## Prerequisites

- **Rust 1.91+** with edition 2024
- **SQLite 3.35+**
- **Qdrant 1.16+** (optional)
- **OpenAI API key** (optional)

## Code Style

- `cargo fmt` before committing
- `cargo clippy` and fix warnings
- No emojis in code/comments

---

## Using Mira Tools (For Claude Code)

Mira provides persistent memory and intelligence that survives across sessions. Use these tools proactively to provide better assistance.

### When to Use Each Tool

**Memory Tools - Use Frequently:**

| Trigger | Action |
|---------|--------|
| User states a preference ("I prefer...", "always use...", "we do X here") | `remember` the preference |
| User makes a decision ("let's go with...", "we decided...") | `remember` the decision |
| User asks about past work ("what did we...", "how did I...") | `recall` to find relevant memories |
| Starting work on a project | `recall` to get context about past sessions |
| User corrects you or says "don't do X" | `remember` the correction |

**Code Intelligence - Before Making Changes:**

| Trigger | Action |
|---------|--------|
| About to modify a file | `get_related_files` to find co-change patterns |
| Need to understand a function | `get_call_graph` to see what calls it and what it calls |
| Looking for code that does X | `semantic_code_search` for natural language search |
| Understanding a file's structure | `get_symbols` to list functions/classes |

**Git Intelligence - For Context:**

| Trigger | Action |
|---------|--------|
| About to change a file | `find_cochange_patterns` to see what else usually changes |
| Encountering an error | `find_similar_fixes` to see if it's been fixed before |
| Fixed a tricky bug | `record_error_fix` so future sessions can learn from it |

**Session Management - For Continuity:**

| Trigger | Action |
|---------|--------|
| End of a significant session | `store_session` with a summary |
| User asks about past sessions | `search_sessions` to find relevant work |
| Recording an architectural decision | `store_decision` for future reference |

**Project Guidelines:**

| Trigger | Action |
|---------|--------|
| User mentions a coding convention | `add_guideline` to record it |
| Starting to write code | `get_guidelines` to check project conventions |

### Parallel Tool Usage

Mira tools can be called in parallel for efficiency. When gathering context, fire multiple calls at once:

```
# Good - parallel calls for context gathering
get_related_files(file_path="src/auth.rs")  \
get_symbols(file_path="src/auth.rs")        \  # All in parallel
recall(query="auth implementation")          /

# Then use results together
```

### Example Workflows

**Starting a New Session:**
```
1. recall(query="recent work on this project") - Get context
2. get_guidelines(project_path=".") - Check conventions
3. list_tasks(status="pending") - See outstanding work
```

**Before Modifying Code:**
```
1. get_related_files(file_path="target.rs") - What else might need changes?
2. get_call_graph(symbol="function_name") - What depends on this?
3. find_cochange_patterns(file_path="target.rs") - Historical patterns
```

**After Fixing a Bug:**
```
1. record_error_fix(error_pattern="the error", fix_description="how it was fixed")
2. remember(content="Fixed X by doing Y", fact_type="context")
```

**When User States a Preference:**
```
User: "We always use Result<T, Error> instead of panicking"
Action: remember(content="Use Result<T, Error> instead of panicking - no unwrap() in production code", fact_type="preference", category="error_handling")
```

**End of Session:**
```
1. store_session(summary="Implemented auth module with JWT tokens, added tests", topics=["auth", "jwt", "testing"])
2. Any pending tasks: update_task() or create_task() for follow-up
```

### Tips for Effective Use

1. **Be Specific in Memories**: "Use snake_case for variables" is better than "naming convention discussed"
2. **Use Categories**: Helps with filtering - "architecture", "style", "preference", "decision"
3. **Record Decisions with Context**: Include WHY, not just WHAT
4. **Check Before Asking**: Use `recall` before asking the user something they may have told you before
5. **Build Knowledge Over Time**: Each `remember` call builds a smarter assistant for future sessions
