# CLAUDE.md

This project uses **Mira** for persistent memory and code intelligence.

## Session Start

```
session_start(project_path="/home/peter/Mira")
```

Then `recall("preferences")` before writing code.

## Code Navigation (Use These First)

**Always prefer Mira tools over Grep/Glob for code exploration:**

| Need | Tool | Why |
|------|------|-----|
| Search by meaning | `semantic_code_search` | Understands intent, not just keywords |
| File structure | `get_symbols` | Functions, structs, classes in a file |
| Check past decisions | `recall` | What we decided and why |
| Codebase overview | `session_start` output | Module map with summaries |

**When to use Grep:** Only for literal string searches (error messages, specific constants, config values).

**When to use Glob:** Only for finding files by exact name pattern.

## Build & Deploy

```bash
cargo build --release
systemctl --user restart mira
```

## Service Management

Mira runs as a systemd user service (auto-starts on boot):

```bash
systemctl --user status mira    # Check status
systemctl --user restart mira   # Restart after rebuild
systemctl --user stop mira      # Stop
systemctl --user start mira     # Start
journalctl --user -u mira -f    # View logs (follow)
journalctl --user -u mira -n 50 # View last 50 lines
```

**URLs:**
- Studio: http://localhost:3000
- Chat (DeepSeek): http://localhost:3000/chat
- Ghost Mode: http://localhost:3000/ghost

## Testing & Debugging

Test chat without UI (requires `mira web` running):

```bash
# Simple test
mira test-chat "Hello, what tools do you have?"

# Verbose (shows reasoning, tool args, results)
mira test-chat -v "Search for authentication code"

# With project context
mira test-chat -p /home/peter/Mira "What are the recent goals?"
```

Note: `test-chat` uses HTTP to call the web server, ensuring messages are stored and background tasks (fact extraction, summarization) run properly.

API endpoint for programmatic testing:

```bash
curl -X POST http://localhost:3000/api/chat/test \
  -H "Content-Type: application/json" \
  -d '{"message":"What is 2+2?","history":[]}'
```

Returns detailed JSON with request_id, duration, reasoning, tool calls, usage stats.

## Tracing

All chat requests include structured logging with:
- `request_id` - UUID for tracking through system
- `duration_ms` - Timing at each stage
- Tool execution with timing and results

View logs: `journalctl --user -u mira -f`

## Environment

API keys are in `/home/peter/Mira/.env`:
- `DEEPSEEK_API_KEY` - Chat/Reasoner
- `OPENAI_API_KEY` - Embeddings (text-embedding-3-small)
