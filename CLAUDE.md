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

## Build & Test

```bash
cargo build --release
```

The binary is at `target/release/mira`. Claude Code spawns it via MCP (configured in `.mcp.json`).

## Debugging

```bash
# Debug session_start output
mira debug-session

# Debug cartographer module detection
mira debug-carto
```

## Environment

API keys are in `/home/peter/Mira/.env`:
- `OPENAI_API_KEY` - Embeddings (text-embedding-3-small)
- `DEEPSEEK_API_KEY` - Expert consultation (Reasoner)
