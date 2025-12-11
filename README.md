# Mira Power Suit

**Memory and Intelligence Layer for Claude Code**

Mira is a "power suit" for Claude Code - it provides persistent memory, code intelligence, git intelligence, and project context through the Model Context Protocol (MCP). Claude Code handles all AI orchestration; Mira provides the superpowers.

## What Mira Does

- **Persistent Memory** - Remember facts, decisions, and preferences across sessions
- **Code Intelligence** - Understand code relationships, call graphs, and symbols
- **Git Intelligence** - Know who's expert on what code, find similar past fixes
- **Project Context** - Store and recall coding guidelines and conventions

## Quick Start

### Build

```bash
cd backend
cargo build --release
```

### Configure Claude Code

Add to `~/.claude/mcp.json`:

```json
{
  "mcpServers": {
    "mira": {
      "command": "/path/to/mira/backend/target/release/mira",
      "env": {
        "DATABASE_URL": "sqlite:///path/to/mira/backend/data/mira.db"
      }
    }
  }
}
```

Restart Claude Code to load the MCP server.

## Available Tools (22)

### Memory Tools
| Tool | Description |
|------|-------------|
| `remember` | Store a fact, decision, or preference for future sessions |
| `recall` | Search through stored memories |
| `forget` | Remove a stored memory by ID |

### Code Intelligence
| Tool | Description |
|------|-------------|
| `get_symbols` | Get functions, classes, structs from a file |
| `get_call_graph` | See what calls a function and what it calls |
| `get_related_files` | Find files related through imports or co-change patterns |

### Git Intelligence
| Tool | Description |
|------|-------------|
| `get_file_experts` | Find developers with expertise on a file |
| `find_similar_fixes` | Search for similar past errors and their fixes |
| `get_change_risk` | Assess risk of changing a file |
| `find_cochange_patterns` | Find files that usually change together |

### Project Context
| Tool | Description |
|------|-------------|
| `get_guidelines` | Get coding guidelines for a project |
| `add_guideline` | Add a coding guideline or convention |

### Session & Data Access
| Tool | Description |
|------|-------------|
| `list_sessions` | List chat sessions |
| `get_session` | Get session details |
| `search_memories` | Search chat message history |
| `get_recent_messages` | Get recent messages from a session |
| `list_operations` | List LLM operations |
| `get_budget_status` | Get API budget usage |
| `get_cache_stats` | Get LLM cache statistics |
| `get_tool_usage` | Get tool execution statistics |
| `list_tables` | List database tables |
| `query` | Execute read-only SQL queries |

## Example Usage

Once configured, you can ask Claude Code things like:

- "Remember that we use snake_case for variables in this project"
- "What coding conventions do we have?"
- "Who should review changes to auth.rs?"
- "Have we seen this type of error before?"
- "What files usually change together with main.rs?"
- "What functions call the parse() function?"

## Architecture

```
Claude Code  <--MCP(stdio)-->  Mira MCP Server
                                    |
                  +----------+------+------+
                  |          |             |
               SQLite     Qdrant      Git Repo
               (facts,    (vectors,   (history,
               sessions)   code)       commits)
```

Mira is a single binary that runs as an MCP server over stdio. Claude Code drives all interactions; Mira provides persistent storage and intelligence capabilities.

## Requirements

- SQLite 3.35+ (embedded)
- Qdrant 1.16+ (optional, for semantic search)
- Rust 1.91+ (if building from source)

## Development

```bash
# Build debug
cargo build

# Run tests
cargo test

# Build release
cargo build --release

# Test MCP server
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}' | DATABASE_URL="sqlite://data/mira.db" ./target/release/mira
```

## Database

Mira uses SQLite for structured data with the following key tables:

- `memory_facts` - Stored memories (remember/recall/forget)
- `project_guidelines` - Project coding conventions
- `code_elements` - Parsed code symbols
- `call_graph` - Function call relationships
- `semantic_edges` - Code semantic relationships
- `file_cochange_patterns` - Git co-change analysis
- `author_expertise` - Developer expertise scores
- `historical_fixes` - Past error fixes

## License

MIT
