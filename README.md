# Mira Power Suit

**Memory and Code Intelligence Layer for Claude Code**

Mira gives Claude Code persistent memory and deep code understanding across sessions. It remembers your preferences, indexes your codebase, and preserves context automatically.

## Quick Install (Docker)

```bash
git clone https://github.com/ConaryLabs/Mira.git ~/.mira
cd ~/.mira
./install.sh
```

Then restart Claude Code.

## Features

### Memory
- **Remembers** your preferences, decisions, and corrections
- **Recalls** past context using semantic search
- **Stores** project-specific conventions and guidelines
- **Tracks** goals and tasks across sessions
- **Permission persistence** - approved tools auto-approve in future sessions

### Code Intelligence
- **Multi-language** - Rust, Python, TypeScript, JavaScript, Go
- **Indexes** code symbols (functions, classes, structs, traits, interfaces)
- **Tracks** imports and dependencies
- **Builds** call graphs with unresolved call tracking for cross-file resolution
- **Parallel indexing** - parses files concurrently for fast indexing
- **Semantic search** - find code by meaning, not just keywords

### Git Intelligence
- **Indexes** commit history with batch transactions
- **Detects** co-change patterns (files that change together)
- **Tracks** file expertise based on commit history
- **Searches** commits by message

### Context Preservation
- **PreCompact hook** - automatically saves context before Claude Code compacts
- **PostToolUse hook** - tracks significant tool outputs for memory
- **Session summaries** - searchable history of past sessions
- **Background daemon** - continuously indexes code changes

## Usage

Add to your project's `CLAUDE.md`:

```markdown
# CLAUDE.md

This project uses Mira for persistent memory.

## Session Start
Call once at the start of every session:
session_start(project_path="/path/to/project")
```

The `session_start` tool sets the project, loads persona, context, corrections, and goals in one call.

Then just talk naturally:
- "Remember that we use snake_case for variables"
- "What did we decide about the auth flow?"
- "Find functions that handle user authentication"
- "What files usually change together with auth.rs?"

## Key Tools

### Session Management
| Tool | Description |
|------|-------------|
| `session_start` | Initialize session: sets project, loads all context (call once at start) |
| `set_project` | Set active project for scoped data |
| `get_project` | Get currently active project |
| `get_session_context` | Get context from previous sessions |
| `store_session` | Store session summary at session end |

### Memory
| Tool | Description |
|------|-------------|
| `remember` | Store a fact, preference, or decision |
| `recall` | Semantic search through memories |
| `forget` | Delete a memory by ID |
| `store_decision` | Store an important decision with context |
| `get_proactive_context` | Get all context for current work |

### Code Intelligence
| Tool | Description |
|------|-------------|
| `index` | Index code/git (actions: project/file/status) |
| `get_symbols` | Get functions/classes from a file |
| `get_call_graph` | See what calls what (with depth traversal) |
| `get_related_files` | Find related files via imports or co-change |
| `semantic_code_search` | Find code by meaning |

### Git Intelligence
| Tool | Description |
|------|-------------|
| `get_recent_commits` | Recent git history, optionally filtered |
| `search_commits` | Search commits by message |
| `find_cochange_patterns` | Files that change together |

### Project Management
| Tool | Description |
|------|-------------|
| `task` | Manage tasks (create/list/get/update/complete/delete) |
| `goal` | Manage goals and milestones |
| `get_guidelines` | Get coding guidelines (use category='mira_usage' for tool guidance) |
| `add_guideline` | Add a coding guideline or convention |

### Learning & Corrections
| Tool | Description |
|------|-------------|
| `correction` | Record/get/validate corrections when user corrects you |
| `record_error_fix` | Record an error fix for future learning |
| `find_similar_fixes` | Find similar past error fixes |
| `record_rejected_approach` | Record approaches to avoid |

### Build & Permissions
| Tool | Description |
|------|-------------|
| `build` | Track builds and errors |
| `permission` | Save/list/delete permission rules for auto-approval |

### Database
| Tool | Description |
|------|-------------|
| `list_tables` | List database tables with row counts |
| `query` | Execute read-only SQL SELECT queries |

## Requirements

- Docker with Docker Compose
- Claude Code
- (Optional) Google Gemini API key for semantic search (free tier available)

## What Gets Installed

The install script sets up:
- **Mira MCP server** - Code intelligence and memory (Docker)
- **Qdrant** - Vector database for semantic search (Docker, port 6334)
- **SQLite** - Persistent storage at `~/.mira/data/mira.db`
- **Hooks** - PreCompact and PostToolUse hooks for auto-context

## Semantic Search

For better recall and code search (finds by meaning, not just keywords), set your Google Gemini API key:

```bash
echo "GEMINI_API_KEY=your-key-here" >> ~/.mira/.env
```

Get a free API key at: https://aistudio.google.com/apikey

This enables gemini-embedding-001 (3072 dimensions) for semantic similarity search across memories and code.

## Parallel Indexing

The `index` tool supports parallel indexing for faster codebase analysis:

```
index(action="project", path="/path/to/project", parallel=true, max_workers=4)
```

- **parallel** (default: true) - Parse files concurrently
- **max_workers** (default: 4) - Maximum concurrent parse workers

The indexer uses a "parallel parse, sequential write" design optimized for SQLite.

## Permission Persistence

When you approve a tool permission, Mira can remember it:

```
permission(action="save", tool_name="Bash", input_field="command",
           input_pattern="cargo ", match_type="prefix")
```

Future sessions can auto-approve matching tool calls via hooks.

## HTTP Server Mode

Run Mira as an HTTP server for multi-device access or Studio integration. Includes MCP, Chat API, and background indexer all on one port.

```bash
# Start HTTP server
mira serve-http --port 3000

# With authentication (recommended)
mira serve-http --port 3000 --auth-token "your-secret"

# Or via environment variable
MIRA_AUTH_TOKEN="your-secret" mira serve-http
```

### Claude Code Configuration

**Important:** Use `http` transport, not `sse`. Mira uses MCP Streamable HTTP.

Via CLI:
```bash
claude mcp add mira -t http http://localhost:3000/mcp \
  -H 'Authorization: Bearer your-secret'
```

Or manually in `~/.claude.json`:
```json
{
  "mcpServers": {
    "mira": {
      "type": "http",
      "url": "http://your-server:3000/mcp",
      "headers": {
        "Authorization": "Bearer your-secret"
      }
    }
  }
}
```

### Health Endpoint

Check server status:
```bash
curl http://localhost:3000/health
```

### Systemd Service

To run as a persistent service:

```bash
# Edit the service file with your settings
sudo cp mira-server.service /etc/systemd/system/

# Set your tokens in the service file
sudo systemctl edit mira-server
# Add:
# [Service]
# Environment=MIRA_AUTH_TOKEN=your-secret
# Environment=GEMINI_API_KEY=your-key

# Enable and start
sudo systemctl enable mira-server
sudo systemctl start mira-server
```

Benefits of HTTP mode:
- **Multi-device** - Connect from phone, laptop, work machine
- **Shared memory** - All sessions share the same database
- **Persistent** - Runs as a service, survives SSH disconnects
- **Remote** - Access your dev box memory from anywhere

## Manual Install (without Docker)

```bash
# Build
SQLX_OFFLINE=true cargo build --release

# Run Qdrant (optional, for semantic search)
docker run -d -p 6333:6333 -p 6334:6334 qdrant/qdrant

# Initialize database
DATABASE_URL="sqlite://data/mira.db" sqlx database create
DATABASE_URL="sqlite://data/mira.db" sqlx migrate run
sqlite3 data/mira.db < seed_mira_guidelines.sql

# Add to ~/.claude/mcp.json
{
  "mcpServers": {
    "mira": {
      "command": "/path/to/mira/target/release/mira",
      "env": {
        "DATABASE_URL": "sqlite:///path/to/mira/data/mira.db",
        "QDRANT_URL": "http://localhost:6334",
        "GEMINI_API_KEY": "your-key-here"
      }
    }
  }
}
```

## Architecture

```
Claude Code  <--MCP-->  Mira Server  -->  SQLite (memories, symbols, commits)
                 |                   -->  Qdrant (semantic vectors)
                 |
        stdio (default) or HTTP (remote)
```

Mira runs as an MCP server with two transport options:
- **stdio** (default) - Claude Code spawns Mira as a subprocess
- **HTTP** - Mira runs as a persistent HTTP server using MCP Streamable HTTP transport

The optional daemon provides background code indexing independent of the MCP server.

## Supported Languages

Code indexing supports:
- **Rust** - functions, structs, enums, traits, impl blocks, modules
- **Python** - functions, classes, imports
- **TypeScript/TSX** - functions, classes, interfaces, type aliases
- **JavaScript/JSX** - functions, classes
- **Go** - functions, methods, structs, interfaces, types

## Project Scoping

Data is scoped to projects:
- **Preferences** (e.g., "I prefer tabs") - Global
- **Decisions** (e.g., "We chose PostgreSQL") - Project-specific
- **Code symbols** - Project-specific
- **Git history** - Project-specific

Call `session_start()` or `set_project()` at session start to enable project-scoped data.

## License

MIT
