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
- **semantic_code_search** - Find code by meaning
- **index** - Index project code for search
- **summarize_codebase** - Generate LLM-powered module descriptions

### Project Management
- **task** - Create/list/update/complete tasks
- **goal** - Track goals with milestones

### Session
- **session_start** - Initialize session with project context
- **set_project** / **get_project** - Manage active project
- **session_history** - Query session and tool call history

### Chat (DeepSeek Integration)
Full conversational memory system at `http://localhost:3000/chat`:
- **Message Storage** - All messages persisted to database
- **Rolling Summaries** - Older messages automatically summarized (L1 session → L2 daily → L3 weekly)
- **Fact Extraction** - Personal facts extracted via LLM and stored globally
- **Semantic Recall** - Context loaded based on message similarity
- **KV Cache Optimization** - Context ordered static-to-volatile for cache efficiency

#### Chat UI Features
- **Project Sidebar** - Slide-out panel to switch between projects
- **Message Bubbles** - User messages right-aligned (orange accent), assistant left-aligned
- **Collapsible Sections** - Thinking and tool calls collapsed by default, expand to view
- **Markdown Rendering** - Full markdown support with code blocks, lists, links
- **Code Blocks** - Syntax highlighting with copy button
- **Streaming Indicators** - Typing dots and thinking spinner during responses

### Ghost Mode (Web UI)
Real-time visualization of Claude Code activity:
- Live tool call streaming via WebSocket
- Session history replay on connect
- Automatic reconnection with sync protocol
- Diff preview with syntax highlighting

Access at `http://localhost:3000` when running `mira web`.

### Background Processing
Automatic idle-time processing for cost savings:
- **Embeddings**: Queued for OpenAI Batch API (50% cheaper)
- **Module Summaries**: Rate-limited DeepSeek calls
- **Real-time Fallback**: Immediate embedding if needed before batch completes

## Architecture

```
┌─────────────────────────────────────────┐
│              Claude Code                │
│                   │                     │
│                   ▼                     │
│         MCP Protocol (stdio)            │
└─────────────────────────────────────────┘
                    │
                    ▼
┌─────────────────────────────────────────┐
│              Mira (mira serve)          │
│                                         │
│   ┌─────────────────────────────────┐  │
│   │         MCP Server (rmcp)       │  │
│   │   session_start, remember,      │  │
│   │   recall, get_symbols, etc.     │  │
│   └──────────────┬──────────────────┘  │
│                  │ broadcast            │
│   ┌──────────────▼──────────────────┐  │
│   │      Web Server (mira web)      │  │
│   │   Ghost Mode UI, WebSocket,     │  │
│   │   REST API, session history     │  │
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
| `mira` or `mira serve` | Run as MCP server (for Claude Code) |
| `mira web` | Run web server with Ghost Mode UI (port 3000) |
| `mira index --path /project` | Index a project's code |
| `mira test-chat "message"` | Test chat via HTTP (requires `mira web`) |

## Configuration

### Environment Variables

| Variable | Required | Description |
|----------|----------|-------------|
| `OPENAI_API_KEY` | For semantic search | OpenAI API key for embeddings |
| `GOOGLE_API_KEY` | Fallback | Alternative to OPENAI_API_KEY |

### Data Storage

All data stored in `~/.mira/mira.db`:
- Memory facts with semantic embeddings
- Code symbols (functions, structs, classes)
- Tasks and goals
- Session history
- Permission rules

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

Simplified schema with 17 tables + 2 vector tables:

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
- `permission_rules` - Auto-approval rules
- `pending_embeddings` - Queue for batch embedding
- `background_batches` - Track active batch jobs
- `chat_messages` - Stored conversation history
- `chat_summaries` - Multi-level conversation summaries

### Vector Tables (sqlite-vec)
- `vec_memory` - Memory embeddings (1536 dimensions)
- `vec_code` - Code chunk embeddings

## Requirements

- Rust toolchain (for building)
- Gemini API key (free tier) for semantic search

## License

MIT
