# Mira Power Suit

**Memory and Intelligence Layer for Claude Code**

Mira gives Claude Code persistent memory across sessions. It remembers your preferences, decisions, and project context so you don't have to repeat yourself.

## Quick Install (Docker)

```bash
git clone https://github.com/ConaryLabs/Mira.git ~/.mira
cd ~/.mira
./install.sh
```

Then restart Claude Code.

## What Mira Does

- **Remembers** your preferences, decisions, and corrections
- **Recalls** past context when relevant
- **Stores** project-specific conventions and guidelines
- **Tracks** what you're working on across sessions

## Usage

Add to your project's `CLAUDE.md`:

```markdown
## Mira Memory
At session start:
set_project(project_path="/path/to/your/project")
get_guidelines(category="mira_usage")
```

Then just talk naturally:
- "Remember that we use snake_case for variables here"
- "What did we decide about the auth flow?"
- "We always run tests before committing"

Mira will automatically store and recall relevant context.

## Key Tools

| Tool | What it does |
|------|--------------|
| `set_project` | Set which project you're working on |
| `remember` | Store a fact, preference, or decision |
| `recall` | Search through stored memories |
| `get_guidelines` | Get project conventions |
| `store_session` | Save a session summary |

## Requirements

- Docker
- Claude Code

## Manual Install (without Docker)

```bash
# Build
cargo build --release

# Initialize database
DATABASE_URL="sqlite://data/mira.db" sqlx migrate run
sqlite3 data/mira.db < seed_mira_guidelines.sql

# Add to ~/.claude/mcp.json
{
  "mcpServers": {
    "mira": {
      "command": "/path/to/mira/target/release/mira",
      "env": {
        "DATABASE_URL": "sqlite:///path/to/mira/data/mira.db"
      }
    }
  }
}
```

## Optional: Semantic Search

For better recall (finds memories by meaning, not just keywords), add Qdrant and OpenAI:

```bash
# Run Qdrant
docker run -d -p 6334:6334 qdrant/qdrant

# Add to environment
export QDRANT_URL="http://localhost:6334"
export OPENAI_API_KEY="sk-..."
```

## How It Works

```
Claude Code  <--MCP-->  Mira
                          |
                       SQLite (memories, guidelines)
                          |
                       Qdrant (optional: semantic search)
```

Mira is a single binary MCP server. Claude Code drives all interactions; Mira provides persistent storage.

## Project Scoping

Memories are scoped to projects:
- **Preferences** (e.g., "I prefer tabs") → Global
- **Decisions** (e.g., "We chose PostgreSQL") → Project-specific
- **Context** (e.g., "The auth module uses JWT") → Project-specific

Call `set_project()` at session start to enable project-scoped data.

## License

MIT
