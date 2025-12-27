# Mira Power Suit

**Memory and Code Intelligence Layer for Claude Code**

Mira gives Claude Code persistent memory and deep code understanding across sessions. It runs as a daemon on your machine, providing semantic search, code intelligence, and context preservation.

## Quick Install (Linux)

```bash
git clone https://github.com/ConaryLabs/Mira.git
cd Mira
./service/install.sh
```

This builds Mira, installs it as a systemd user service, and starts the daemon on port 3199.

Then add to `~/.claude/settings.local.json`:
```json
{
  "mcpServers": {
    "mira": {
      "command": "/usr/local/bin/mira",
      "args": ["connect"]
    }
  }
}
```

Restart Claude Code and you're ready.

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    MIRA DAEMON                          │
│              (always running on port 3199)              │
│                                                         │
│   ┌─────────────────────────────────────────────────┐  │
│   │              SHARED CORE                         │  │
│   │  • All MCP tools (42 tools)                     │  │
│   │  • Semantic search (Qdrant + Gemini)            │  │
│   │  • Background indexer                           │  │
│   │  • SQLite persistence                           │  │
│   └─────────────────────────────────────────────────┘  │
│                         │                               │
│          ┌──────────────┴──────────────┐               │
│          ▼                             ▼               │
│   MCP endpoint                    HTTP API             │
│   (/mcp)                         (/api/chat/*)         │
│   for Claude Code                for Studio            │
└─────────────────────────────────────────────────────────┘
           │                             │
           ▼                             ▼
    mira connect                   Mira Studio
    (stdio shim)                  (SvelteKit web UI)
```

## Mira Studio

A web-based chat interface for Mira. Provides a modern terminal-style UI with:
- Streaming chat with DeepSeek V3.2
- Live streaming status showing tool names and token counts
- Message role labels (`[you]`/`[mira]`) with hover timestamps
- Tool call visualization with Timeline panel
- Project management with pinnable project cards
- File artifact tracking with Workspace panel
- Structured tool argument display (key-value, not raw JSON)
- Mobile-responsive with bottom sheet panels

```bash
cd studio
npm install
npm run dev
# Open http://localhost:5173
```

See [studio/README.md](studio/README.md) for full documentation.

The daemon provides:
- **MCP endpoint** at `/mcp` for Claude Code integration
- **Chat API** at `/api/chat/*` for Mira Studio
- **Background indexer** for continuous code intelligence
- **Health endpoint** at `/health` for monitoring

## Commands

| Command | Description |
|---------|-------------|
| `mira` | Start daemon in foreground (default) |
| `mira daemon` | Start daemon (same as above) |
| `mira connect` | Stdio shim for Claude Code |
| `mira status` | Check daemon health |
| `mira stop` | Show stop instructions |

## Service Management

```bash
# View service status
systemctl --user status mira

# Restart daemon
systemctl --user restart mira

# View logs
journalctl --user -u mira -f

# Stop daemon
systemctl --user stop mira

# Uninstall
./service/uninstall.sh
```

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
- **Builds** call graphs with cross-file resolution
- **Semantic search** - find code by meaning, not just keywords

### Git Intelligence
- **Indexes** commit history
- **Detects** co-change patterns (files that change together)
- **Tracks** file expertise based on commit history
- **Searches** commits by message

### Context Preservation
- **PreCompact hook** - saves context before Claude Code compacts
- **PostToolUse hook** - tracks significant tool outputs
- **Session summaries** - searchable history of past sessions
- **Background indexer** - continuously indexes code changes

## Configuration

Environment variables are loaded from `~/.mira/.env`:

```bash
# Required
DATABASE_URL=sqlite:///home/you/.mira/mira.db

# For semantic search (recommended)
QDRANT_URL=http://localhost:6334
GEMINI_API_KEY=your-gemini-key

# For chat
DEEPSEEK_API_KEY=your-deepseek-key

# For web search in chat
GOOGLE_SEARCH_API_KEY=your-google-key
GOOGLE_SEARCH_CX=your-search-engine-id
```

Get a free Gemini API key at: https://aistudio.google.com/apikey

## Usage

Add to your project's `CLAUDE.md`:

```markdown
# CLAUDE.md

This project uses Mira for persistent memory.

## Session Start
Call once at the start of every session:
session_start(project_path="/path/to/project")
```

Then just talk naturally:
- "Remember that we use snake_case for variables"
- "What did we decide about the auth flow?"
- "Find functions that handle user authentication"
- "What files usually change together with auth.rs?"

## Key Tools

### Session Management
| Tool | Description |
|------|-------------|
| `session_start` | Initialize session: sets project, loads all context |
| `set_project` | Set active project for scoped data |
| `get_session_context` | Get context from previous sessions |
| `store_session` | Store session summary at session end |

### Memory
| Tool | Description |
|------|-------------|
| `remember` | Store a fact, preference, or decision |
| `recall` | Semantic search through memories |
| `store_decision` | Store an important decision with context |
| `get_proactive_context` | Get all context for current work |

### Code Intelligence
| Tool | Description |
|------|-------------|
| `index` | Index code/git (actions: project/file/status) |
| `get_symbols` | Get functions/classes from a file |
| `get_call_graph` | See what calls what |
| `get_related_files` | Find related files via imports or co-change |
| `semantic_code_search` | Find code by meaning |

### Git Intelligence
| Tool | Description |
|------|-------------|
| `get_recent_commits` | Recent git history |
| `search_commits` | Search commits by message |
| `find_cochange_patterns` | Files that change together |

### Project Management
| Tool | Description |
|------|-------------|
| `task` | Manage tasks (create/list/get/update/complete/delete) |
| `goal` | Manage goals and milestones |
| `correction` | Record corrections when user corrects you |

## Requirements

- Linux with systemd
- Rust toolchain (for building)
- Qdrant (for semantic search) - `docker run -d -p 6334:6334 qdrant/qdrant`
- Gemini API key (free tier available)

## Supported Languages

Code indexing supports:
- **Rust** - functions, structs, enums, traits, impl blocks
- **Python** - functions, classes, imports
- **TypeScript/TSX** - functions, classes, interfaces
- **JavaScript/JSX** - functions, classes
- **Go** - functions, methods, structs, interfaces

## Project Scoping

Data is scoped to projects:
- **Preferences** - Global across all projects
- **Decisions** - Project-specific
- **Code symbols** - Project-specific
- **Git history** - Project-specific

Call `session_start()` at session start to enable project-scoped data.

## Health Check

```bash
curl http://localhost:3199/health
```

Returns:
```json
{
  "status": "ok",
  "version": "2.0.0",
  "database": "ok",
  "semantic_search": "ok",
  "port": 3199
}
```

## Troubleshooting

### MCP Connection Fails ("Failed to reconnect to mira")

**Symptoms:**
- `/mcp` shows `mira ✘ failed`
- Claude Code logs show "Starting Mira Daemon on port 3000" then "Address already in use"

**Diagnosis steps:**

1. **Check daemon is running:**
   ```bash
   systemctl --user status mira
   # Should show "active (running)"
   ```

2. **Check MCP config is correct:**
   ```bash
   cat ~/.mcp.json
   # Or for project-specific:
   cat /path/to/project/.mcp.json
   ```
   Should contain:
   ```json
   {
     "mcpServers": {
       "mira": {
         "command": "/usr/local/bin/mira",
         "args": ["connect"]
       }
     }
   }
   ```

3. **Test connect command manually:**
   ```bash
   echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}' | /usr/local/bin/mira connect
   ```
   Should return JSON with `serverInfo`.

4. **Check Claude Code MCP logs:**
   ```bash
   ls ~/.cache/claude-cli-nodejs/*/mcp-logs-mira/
   cat ~/.cache/claude-cli-nodejs/*/mcp-logs-mira/$(ls -t ~/.cache/claude-cli-nodejs/*/mcp-logs-mira/ | head -1)
   ```
   Look for "Starting Mira Daemon" - this means wrong command is being used.

**Common causes & fixes:**

| Issue | Cause | Fix |
|-------|-------|-----|
| "Starting Mira Daemon" in logs | Claude Code cached old config | Quit Claude Code completely, restart |
| "Address already in use" | Daemon running but connect not used | Check `.mcp.json` has `args: ["connect"]` |
| "Cannot connect to Mira daemon" | Daemon not running | `systemctl --user start mira` |
| Wrapper not being called | Claude Code cached old config | `/mcp disable mira` then `/mcp enable mira` |

**Nuclear option - force refresh:**
```bash
# 1. Quit Claude Code completely
# 2. Clear MCP cache
rm -rf ~/.cache/claude-cli-nodejs/*/mcp-logs-mira/
# 3. Restart daemon
systemctl --user restart mira
# 4. Start Claude Code fresh
```

### Port Mismatch

The daemon port should match between:
- `~/.config/systemd/user/mira.service` (Environment=MIRA_PORT=...)
- `~/.mira/.env` (MIRA_PORT=...)
- What `mira connect` expects (default: 3000)

Check current port:
```bash
curl http://localhost:3000/health
# Or check service:
journalctl --user -u mira | grep "listening on"
```

### Semantic Search Not Working

Check Qdrant is running:
```bash
curl http://localhost:6334/health
```

Check Gemini key is set in `~/.mira/.env`:
```bash
grep GEMINI ~/.mira/.env
```

## License

MIT
