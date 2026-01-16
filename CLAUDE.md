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

### Wrong vs Right

| Task | ❌ Wrong | ✓ Right |
|------|----------|---------|
| Find authentication code | `grep -r "auth"` | `semantic_code_search("authentication")` |
| What calls this function? | `grep -r "function_name"` | `find_callers("function_name")` |
| List functions in file | `grep "fn " file.rs` | `get_symbols(file_path="file.rs")` |
| Check if feature exists | `grep -r "feature"` | `check_capability("feature description")` |
| Find config files | `find . -name "*.toml"` | `glob("**/*.toml")` - OK, exact pattern |

## Task Management

Use Mira's `task` and `goal` tools instead of TodoWrite for **cross-session persistence**:

- `task(action="create", title="...")` - persists across sessions
- `goal(action="create", title="...")` - for larger milestones
- `task(action="list")` - see what's pending from previous sessions

**TodoWrite** is fine for ephemeral, single-session checklists. Use Mira tasks when work spans multiple sessions.

## Memory

Use `remember` to store decisions and context for future sessions:

```
remember(content="Decided to use X approach because Y", category="decision")
remember(content="User prefers Z style", category="preference")
```

Then `recall("relevant query")` retrieves it later.

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

## rust-analyzer LSP Plugin

The `rust-analyzer@claude-code-lsps` plugin is enabled in `~/.claude/settings.json`. It provides **passive background intelligence** - not directly callable tools.

**What it does:**
- Automatic diagnostics after file edits (type errors, unused variables, etc.)
- Fix suggestions inline with errors
- Surfaced via `<new-diagnostics>` in system reminders

**Mira vs LSP:**

| Capability | Mira | LSP |
|------------|------|-----|
| Invocation | Explicit tool calls | Automatic after edits |
| Diagnostics | No | Yes, with fix suggestions |
| Semantic search | Yes | No |
| Memory/context | Yes | No |

**Usage:** Just edit `.rs` files normally. Diagnostics appear automatically if there are errors. No explicit invocation needed.

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

## Claude Code Config Locations

| File | Purpose | Scope |
|------|---------|-------|
| `~/.claude.json` | Claude Code state (per-project settings, disabled servers, stats) | Global |
| `~/.claude/settings.json` | User settings (hooks, plugins, thinking mode) | Global |
| `~/.claude/settings.local.json` | Local overrides (not synced) | Global |
| `~/.claude/mcp.json` | Global MCP servers (use sparingly) | Global |
| `<project>/.mcp.json` | Project MCP servers (preferred) | Project |
| `<project>/CLAUDE.md` | Project instructions for Claude | Project |

**Best practices:**
- Define MCP servers in project `.mcp.json`, not global `~/.claude/mcp.json`
- Use `~/.claude/settings.json` for hooks and plugins
- Project-specific overrides go in project's `.mcp.json` or `CLAUDE.md`
