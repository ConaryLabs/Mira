# CLAUDE.md

This project uses **Mira** for persistent memory and code intelligence.

## Session Start

```
session_start(project_path="/home/peter/Mira")
```

Then `recall("preferences")` before writing code.

## CRITICAL: Tool Selection

STOP before using Grep or Glob. Use Mira tools instead:

- `semantic_code_search` - for ANY code search (not Grep)
- `get_symbols` - for file structure (not Grep)
- `find_callers` / `find_callees` - for call graph (not Grep)
- `recall` - for past decisions and preferences
- `check_capability` - to find if something exists in the codebase

**Only use Grep** for literal strings (error messages, UUIDs, specific constants).
**Only use Glob** for exact filename patterns when you know the name.

## Expert Consultation

Use experts for second opinions before major decisions:

- `consult_architect` - system design, patterns, tradeoffs
- `consult_plan_reviewer` - validate plans before coding
- `consult_code_reviewer` - find bugs, quality issues
- `consult_security` - vulnerabilities, hardening
- `consult_scope_analyst` - missing requirements, edge cases

## Code Navigation Reference

| Need | Tool |
|------|------|
| Search by meaning | `semantic_code_search` |
| File structure | `get_symbols` |
| What calls X? | `find_callers` |
| What does X call? | `find_callees` |
| Past decisions | `recall` |
| Feature exists? | `check_capability` |
| Codebase overview | `session_start` output |

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
