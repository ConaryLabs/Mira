# CLAUDE.md

Mira is a Rust MCP server providing persistent memory and code intelligence for Claude Code.

## Session Start

Project context is **auto-initialized** from Claude Code's working directory.
For full session context, call `session(action="recap")`. Use `memory(action="recall", query="preferences")` before writing code.

**Automatic bridging:** Mira hooks capture session source (`startup` vs `resume`), pending tasks, and working directory. Session history shows `[startup]` or `[resume←previous_id]`.

## Anti-Patterns

**NEVER** do these in the Mira codebase:

| Don't | Do Instead |
|-------|------------|
| Use `Database` directly | Use `DatabasePool` for all database access |
| Store secrets in memories | Keep secrets in `.env` only |
| Guess at MCP tool parameters | Check tool schema or existing usage first |
| Add dependencies without checking | Run `memory(action="recall", query="dependencies")` first |
| Change tool handler signatures in `tools/mcp.rs` | Coordinate changes across all tool modules in `tools/core/` |

## Tool Selection

STOP before using Grep or Glob. Prefer Mira tools for semantic work:
- **Code by intent** -> `code(action="search", query="...")` (not Grep)
- **File structure** -> `code(action="symbols", file_path="...")` (not grepping for definitions)
- **Call graph** -> `code(action="callers", ...)` / `code(action="callees", ...)` (not grepping function names)
- **Past decisions** -> `memory(action="recall", query="...")` before architectural changes
- **External libraries** -> Context7: `resolve-library-id` then `query-docs`

Use Grep/Glob only for **literal strings**, **exact filename patterns**, or **simple one-off searches**.

See `.claude/rules/tool-selection.md` for the full decision guide.

## Code Navigation Quick Reference

| Need | Tool |
|------|------|
| Search by meaning | `code(action="search", query="...")` |
| File structure | `code(action="symbols", file_path="...")` |
| What calls X? | `code(action="callers", function_name="...")` |
| What does X call? | `code(action="callees", function_name="...")` |
| Past decisions | `memory(action="recall", query="...")` |
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

API keys are in `~/.mira/.env` (optional with MCP Sampling):
- `DEEPSEEK_API_KEY` - Expert consultation (Reasoner)
- `GEMINI_API_KEY` / `GOOGLE_API_KEY` - Embeddings (Google gemini-embedding-001)

Optional: `BRAVE_API_KEY` (web search), `DEFAULT_LLM_PROVIDER`, `MIRA_DISABLE_LLM`, `MIRA_USER_ID`.
See `.env.example` for all options.

If no keys are configured, experts use MCP Sampling to route through the host client.

## Claude Code Config Locations

| File | Purpose | Scope |
|------|---------|-------|
| `~/.claude.json` | Claude Code state | Global |
| `~/.claude/settings.json` | User settings (hooks, plugins) | Global |
| `~/.claude/mcp.json` | Global MCP servers | Global |
| `<project>/.mcp.json` | Project MCP servers (preferred) | Project |
| `<project>/CLAUDE.md` | Project instructions | Project |

## Mira Skills (Slash Commands)

| Command | Purpose |
|---------|---------|
| `/mira:goals` | List and manage cross-session goals and milestones |
| `/mira:recap` | Get session context, preferences, and active goals |
| `/mira:search <query>` | Semantic code search by concept |
| `/mira:remember <content>` | Quick memory storage |
| `/mira:insights` | Surface background analysis and predictions |
| `/mira:experts <question>` | Get second opinions from AI experts |
| `/mira:diff` | Semantic analysis of code changes |

## Hook Integration

Mira hooks **automatically inject context** — don't manually duplicate this:

| Hook | What It Injects |
|------|-----------------|
| `SessionStart` | Session ID, startup vs resume, working directory |
| `UserPromptSubmit` | Pending tasks, relevant memories, file-aware context |
| `PostToolUse` | Tracks file modifications (async, non-blocking) |
| `PreCompact` | Preserves important context before summarization |
| `Stop` | Session snapshot for continuity |

**Don't:** Manually inject session info, pending tasks, or file tracking that hooks already provide.

## What NOT to Do

Beyond the anti-patterns above, avoid:
- Manually fetching session context that `UserPromptSubmit` hook already injects
- Creating memories for ephemeral info (hooks track file access automatically)
- Duplicating goal/task state between Claude tasks and Mira goals
