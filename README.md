# Mira

**Memory and Code Intelligence Layer for Claude Code**

Mira provides persistent semantic memory and code intelligence for Claude Code via MCP (Model Context Protocol). All data is stored locally in SQLite with sqlite-vec for vector embeddings.

## Quick Start

```bash
# Build
cargo build --release

# Add to your project's .mcp.json
{
  "mcpServers": {
    "mira": {
      "command": "/path/to/mira",
      "args": ["serve"]
    }
  }
}
```

Set `OPENAI_API_KEY` for semantic search (embeddings).

## Features

### Memory
- **remember** - Store facts, decisions, preferences
- **recall** - Semantic search through memories
- **forget** - Delete memories by ID

### Code Intelligence
- **get_symbols** - Extract functions, structs, classes from files
- **semantic_code_search** - Find code by meaning (hybrid semantic + keyword search)
- **find_callers** - Find all functions that call a given function (uses call graph)
- **find_callees** - Find all functions called by a given function
- **check_capability** - Check if a feature exists in codebase (searches cached capabilities, falls back to live code search)
- **index** - Index project code for search
- **summarize_codebase** - Generate LLM-powered module descriptions

### Project Management
- **task** - Create/list/update/complete tasks (supports bulk_create)
- **goal** - Track goals with milestones (supports bulk_create)

### Session
- **session_start** - Initialize session with project context
- **set_project** / **get_project** - Manage active project
- **get_session_recap** - Get session recap (pending tasks, active goals, recent sessions)
- **session_history** - Query session and tool call history

### Expert Consultation
- **consult_architect** - System design, patterns, tradeoffs (uses DeepSeek Reasoner)
- **consult_code_reviewer** - Find bugs, quality issues, improvements
- **consult_security** - Identify vulnerabilities and attack vectors
- **consult_scope_analyst** - Find missing requirements and edge cases
- **consult_plan_reviewer** - Validate implementation plans

## Architecture

```
┌─────────────────────────────────────────┐
│              Claude Code                │
│                   │                     │
│                   ▼                     │
│       MCP Protocol (stdio transport)    │
└─────────────────────────────────────────┘
                   │
                   ▼
┌─────────────────────────────────────────┐
│           Mira (mira serve)             │
│                                         │
│   ┌─────────────────────────────────┐  │
│   │  MCP Server (rmcp)              │  │
│   │   session_start, remember,      │  │
│   │   recall, get_symbols, etc.     │  │
│   └──────────────┬──────────────────┘  │
│                  │                      │
│   ┌──────────────┴──────────────────┐  │
│   │        Background Worker        │  │
│   │   embeddings, summaries,        │  │
│   │   capabilities scan             │  │
│   └─────────────────────────────────┘  │
│                    │                    │
│   ┌────────────────┴────────────────┐  │
│   ▼                                 ▼  │
│ SQLite                         sqlite-vec
│ (rusqlite)                    (embeddings)
│                                         │
│   ~/.mira/mira.db                      │
└─────────────────────────────────────────┘
```

## Commands

| Command | Description |
|---------|-------------|
| `mira serve` | Run as MCP server (default, for Claude Code) |
| `mira index --path /project` | Index a project's code |
| `mira hook permission` | Permission hook for Claude Code |
| `mira debug-carto` | Debug cartographer module detection |
| `mira debug-session` | Debug session_start output |

## Configuration

### Environment Variables

| Variable | Required | Description |
|----------|----------|-------------|
| `OPENAI_API_KEY` | Yes | OpenAI API key for embeddings |
| `DEEPSEEK_API_KEY` | For experts | DeepSeek API key for expert consultation |

### Data Storage

All data stored in `~/.mira/mira.db`:
- Memory facts with semantic embeddings
- Code symbols (functions, structs, classes)
- Tasks and goals
- Session history

## Supported Languages

Code indexing via tree-sitter:
- **Rust** - functions, structs, enums, traits, impl blocks
- **Python** - functions, classes
- **TypeScript/TSX** - functions, classes, interfaces
- **JavaScript/JSX** - functions, classes
- **Go** - functions, methods, structs, interfaces

## Usage

Add to your project's `CLAUDE.md`:

```markdown
## Session Start

Call once at the start of every session:
```
session_start(project_path="/path/to/project")
```
```

Then use naturally:
- "Remember that we use snake_case for variables"
- "What did we decide about the auth flow?"
- "Find functions that handle user authentication"

## Database Schema

Simplified schema with 19 tables + 2 vector tables:

### Core Tables
- `projects` - Project paths and names
- `memory_facts` - Semantic memories with embeddings
- `corrections` - Style/approach corrections
- `code_symbols` - Indexed code symbols
- `call_graph` - Function call relationships
- `imports` - Import/dependency tracking
- `codebase_modules` - Module structure with LLM summaries
- `sessions` - Session history
- `tool_history` - MCP tool call history
- `goals` - High-level goals
- `milestones` - Goal milestones
- `tasks` - Task tracking
- `pending_embeddings` - Queue for batch embedding
- `background_batches` - Track active batch jobs

### Vector Tables (sqlite-vec)
- `vec_memory` - Memory embeddings (1536 dimensions)
- `vec_code` - Code chunk embeddings

## Requirements

- Rust toolchain (for building)
- OpenAI API key for embeddings (text-embedding-3-small)
- DeepSeek API key for expert consultation (optional)

## License

MIT
