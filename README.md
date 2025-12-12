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

### Code Intelligence
- **Indexes** code symbols (functions, classes, structs, traits)
- **Tracks** imports and dependencies
- **Builds** call graphs for impact analysis
- **Semantic search** - find code by meaning, not just keywords

### Git Intelligence
- **Indexes** commit history
- **Detects** co-change patterns (files that change together)
- **Tracks** file expertise based on commit history

### Context Preservation
- **PreCompact hook** - automatically saves context before Claude Code compacts
- **Session summaries** - searchable history of past sessions
- **Background daemon** - continuously indexes code changes

## Usage

Add to your project's `CLAUDE.md`:

```markdown
# CLAUDE.md

This project uses Mira for persistent memory.

## Session Start
Call these at the start of every session:
get_guidelines(category="mira_usage")
get_session_context()
```

Then just talk naturally:
- "Remember that we use snake_case for variables"
- "What did we decide about the auth flow?"
- "Find functions that handle user authentication"
- "What files usually change together with auth.rs?"

## Key Tools

| Tool | What it does |
|------|--------------|
| `set_project` | Set active project for scoped data |
| `remember` | Store a fact, preference, or decision |
| `recall` | Semantic search through memories |
| `get_session_context` | Get context from previous sessions |
| `get_symbols` | Get functions/classes from a file |
| `get_call_graph` | See what calls what |
| `semantic_code_search` | Find code by meaning |
| `get_recent_commits` | Recent git history |
| `find_cochange_patterns` | Files that change together |
| `index` | Index a file or project |
| `goal` | Manage goals and milestones |
| `task` | Track tasks |

## Requirements

- Docker with Docker Compose
- Claude Code
- (Optional) Google Gemini API key for semantic search (free tier available)

## What Gets Installed

The install script sets up:
- **Mira MCP server** - Code intelligence and memory (Docker)
- **Qdrant** - Vector database for semantic search (Docker, port 6334)
- **SQLite** - Persistent storage at `~/.mira/data/mira.db`
- **Hooks** - PreCompact hook for auto-saving context

## Semantic Search

For better recall and code search (finds by meaning, not just keywords), set your Google Gemini API key:

```bash
echo "GEMINI_API_KEY=your-key-here" >> ~/.mira/.env
```

Get a free API key at: https://aistudio.google.com/apikey

This enables gemini-embedding-001 (3072 dimensions) for semantic similarity search across memories and code.

## Daemon Mode (Optional)

For continuous background indexing, run Mira as a daemon:

```bash
# Start daemon (indexes code changes in real-time)
~/.mira/mira daemon start -p /path/to/your/project

# Check status
~/.mira/mira daemon status -p /path/to/your/project

# Stop
~/.mira/mira daemon stop -p /path/to/your/project
```

The daemon:
- Watches for file changes and re-indexes automatically
- Syncs git history periodically
- Generates embeddings for semantic code search

## Manual Install (without Docker)

```bash
# Build
cargo build --release

# Run Qdrant (optional, for semantic search)
docker run -d -p 6333:6333 -p 6334:6334 qdrant/qdrant

# Initialize database
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
Claude Code  <--MCP-->  Mira Server
                           |
                        SQLite (memories, symbols, commits)
                           |
                        Qdrant (semantic vectors for code + memories)
```

Mira runs as an MCP server invoked by Claude Code. The optional daemon provides background indexing.

## Project Scoping

Data is scoped to projects:
- **Preferences** (e.g., "I prefer tabs") - Global
- **Decisions** (e.g., "We chose PostgreSQL") - Project-specific
- **Code symbols** - Project-specific
- **Git history** - Project-specific

Call `set_project()` at session start to enable project-scoped data.

## License

MIT
