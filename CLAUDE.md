# CLAUDE.md

Mira is a Rust MCP server providing persistent memory and code intelligence for Claude Code.

## Session Start

Project context is **auto-initialized** from Claude Code's working directory.
For full session context, call `get_session_recap()`. Use `recall("preferences")` before writing code.

## Anti-Patterns

**NEVER** do these in the Mira codebase:

| Don't | Do Instead |
|-------|------------|
| Use `Database` directly | Use `DatabasePool` for all database access |
| Store secrets in memories | Keep secrets in `.env` only |
| Guess at MCP tool parameters | Check tool schema or existing usage first |
| Add dependencies without checking | Run `recall("dependencies")` first |
| Modify `proxy.rs` handler signatures | Coordinate changes across all tool handlers |

## Tool Selection

STOP before using Grep or Glob. Prefer Mira tools for semantic work:
- **Code by intent** -> `search_code` (not Grep)
- **File structure** -> `get_symbols` (not grepping for definitions)
- **Call graph** -> `find_callers` / `find_callees` (not grepping function names)
- **Past decisions** -> `recall` before architectural changes
- **External libraries** -> Context7: `resolve-library-id` then `query-docs`

Use Grep/Glob only for **literal strings**, **exact filename patterns**, or **simple one-off searches**.

See `.claude/rules/tool-selection.md` for the full decision guide.

## Code Navigation Quick Reference

| Need | Tool |
|------|------|
| Search by meaning | `search_code` |
| File structure | `get_symbols` |
| What calls X? | `find_callers` |
| What does X call? | `find_callees` |
| Past decisions | `recall` |
| Codebase overview | `project(action="start")` output |
| External library API | Context7: `resolve-library-id` -> `query-docs` |
| Literal string search | `Grep` (OK) |
| Exact filename pattern | `Glob` (OK) |

## Build & Test

```bash
cargo build --release
cargo test
```

The binary is at `target/release/mira`. Claude Code spawns it via MCP (configured in `.mcp.json`).

## Debugging

```bash
mira debug-session   # Debug project(action="start") output
mira debug-carto     # Debug cartographer module detection
```

## Environment

API keys are in `~/.mira/.env`:
- `DEEPSEEK_API_KEY` - Expert consultation (Reasoner)
- `GEMINI_API_KEY` - Embeddings (Google gemini-embedding-001)

## Claude Code Config Locations

| File | Purpose | Scope |
|------|---------|-------|
| `~/.claude.json` | Claude Code state | Global |
| `~/.claude/settings.json` | User settings (hooks, plugins) | Global |
| `~/.claude/mcp.json` | Global MCP servers | Global |
| `<project>/.mcp.json` | Project MCP servers (preferred) | Project |
| `<project>/CLAUDE.md` | Project instructions | Project |
