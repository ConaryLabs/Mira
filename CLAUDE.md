# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

Mira is a Rust MCP server providing code intelligence and session management for Claude Code. It runs locally over stdio, persists state in two SQLite databases (`~/.mira/`), and injects context into Claude Code via lifecycle hooks.

## Session Start

Project context is **auto-initialized** from Claude Code's working directory.
For full session context, call `session(action="recap")`.

**Automatic bridging:** Mira hooks capture session source (`startup` vs `resume`), pending tasks, and working directory. Session history shows `[startup]` or `[resume←previous_id]`.

**Notation:** `tool(action="x", param="y")` refers to MCP tool calls. For example, `code(action="search", query="...")` calls the `code` MCP tool with `action="search"`. These are not shell commands.

## Architecture

### Workspace Layout

Cargo workspace with two crates:
- **`crates/mira-server/`** — Main server binary. All application logic lives here.
- **`crates/mira-types/`** — Shared types (`ProjectContext`, `MemoryFact`). Kept minimal.
- **`plugin/`** — Claude Code plugin distribution (hooks, skills, bin, MCP config).

### Request Flow

```
Claude Code → stdio → rmcp framework → MiraServer (ServerHandler)
  → mcp/handler.rs (protocol lifecycle: call_tool, list_tools, resources)
  → mcp/router.rs (#[tool] macro routes to tool handlers)
  → tools/core/<tool>.rs (business logic per tool)
  → db/ (SQLite via DatabasePool)
```

Tools are defined with `#[tool]` attributes in `mcp/router.rs`. Each tool delegates to a module in `tools/core/` (e.g., `tools/core/code/`, `tools/core/goals.rs`). Request/response types live in `mcp/requests.rs` and `mcp/responses/`. When adding or modifying tools, coordinate changes across all three locations.

### Two-Database Architecture

- **`mira.db`** — Sessions, goals, memories, behavior patterns. Access via `ctx.pool()`.
- **`mira-code.db`** — Code index: symbols, call graph, embeddings, FTS. Access via `ctx.code_pool()`.

Always use `DatabasePool` (via `pool.run()` or `pool.interact()`), never `Database`/`Connection` directly.

- `pool.run()` — For MCP tool handlers (auto-converts to `MiraError`)
- `pool.interact()` — For background tasks, migrations (returns `anyhow::Result`)

### Key Subsystems

| Subsystem | Location | Purpose |
|-----------|----------|---------|
| Hooks | `hooks/` | Claude Code lifecycle hooks (session, prompt, tool use, stop) |
| Context | `context/` | Proactive context injection for hooks |
| Indexer | `indexer/` | Tree-sitter code parsing and symbol extraction |
| Cartographer | `cartographer/` | Codebase mapping and module detection |
| Search | `search/` | Semantic + keyword + cross-reference search |
| Background | `background/` | Async workers (embeddings, summaries, health) |
| Mux | `mux/` | Multiplexer pipeline for hook IPC (hooks try `mux.sock` first, fall back to direct IPC) |
| IPC | `ipc/` | Inter-process communication for hook ↔ server |

### MCP Framework

Mira uses the **`rmcp`** crate for MCP protocol implementation. `MiraServer` implements `ServerHandler`. Tools use `#[tool]` and `#[tool_router]` proc macros. Responses use structured output with `output_schema`.

## Build & Test

```bash
cargo build                    # Debug build (NEVER use --release during development)
cargo test                     # Run all tests (NEVER use --release)
cargo test test_name           # Run a specific test
cargo test -- --nocapture      # Run tests with stdout visible
cargo clippy --all-targets     # Lint
cargo fmt                      # Format
```

The binary is at `target/debug/mira`. Claude Code spawns it via MCP (configured in `.mcp.json`). After rebuilding, restart Claude Code to pick up changes.

## Debugging

```bash
mira debug-session   # Debug project(action="start") output
mira debug-carto     # Debug cartographer module detection
```

## Anti-Patterns

**NEVER** do these in the Mira codebase:

| Don't | Do Instead |
|-------|------------|
| Use `Database`/`Connection` directly | Use `DatabasePool` (`pool.run()` or `pool.interact()`) |
| Store secrets in code or config | Keep secrets in `~/.mira/.env` only |
| Guess at MCP tool parameters | Check tool schema or existing usage first |
| Change tool handler signatures in `mcp/router.rs` alone | Coordinate across `router.rs`, `requests.rs`, and `tools/core/` |
| Use `cargo build --release` or `cargo test --release` | Always use debug mode during development |

## Tool Selection

STOP before using Grep or Glob. Prefer Mira tools for semantic work:
- **Code by intent** -> `code(action="search", query="...")` (not Grep)
- **File structure** -> `code(action="symbols", file_path="...")` (not grepping for definitions)
- **Call graph** -> `code(action="callers", ...)` / `code(action="callees", ...)` (not grepping function names)
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
| Codebase overview | `project(action="start")` output |
| External library API | Context7: `resolve-library-id` -> `query-docs` |
| Literal string search | `Grep` (OK) |
| Exact filename pattern | `Glob` (OK) |

## Environment

API keys are in `~/.mira/.env` (all optional):
- `OPENAI_API_KEY` - Embeddings (OpenAI text-embedding-3-small). Without this, search falls back to keyword/fuzzy matching.

Optional: `MIRA_USER_ID`.
See `.env.example` for all options.

## Commit Style

Conventional commits: `feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`, `style:`.

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
| `/mira:insights` | Surface background analysis and predictions |
| `/mira:diff` | Semantic analysis of code changes |
| `/mira:experts` | Expert consultation via Agent Teams |
| `/mira:full-cycle` | End-to-end expert review with implementation and QA |
| `/mira:qa-hardening` | Production readiness review and hardening backlog |
| `/mira:refactor` | Safe code restructuring with architect + reviewer validation |
| `/mira:help` | Show all available Mira commands and tools |
| `/mira:status` | Quick health check: index stats, storage, active goals |

## Hook Integration

Mira hooks **automatically inject context** — don't manually duplicate this:

| Hook | What It Injects |
|------|-----------------|
| `SessionStart` | Session ID, startup vs resume, task list ID, working directory; on resume: previous session context, goals, team info, incomplete tasks from previous session |
| `UserPromptSubmit` | Pending tasks, team context, reactive code intelligence |
| `PreToolUse` | File reread advisory and symbol hints for Read |
| `PostToolUse` | Tracks file modifications, team conflict detection |
| `PreCompact` | Extracts decisions, TODOs, and errors from transcript before summarization |
| `Stop` | Session snapshot, task export, goal progress check |
| `SessionEnd` | Snapshot tasks on user interrupt, team session cleanup |
| `SubagentStart` | Injects goals, project map, and search hints for subagent context |
| `SubagentStop` | Captures discoveries from subagent work |
| `PostToolUseFailure` | Tracks tool failures |
| `TaskCompleted` | Logs task completions, auto-completes matching goal milestones |
| `TeammateIdle` | Logs teammate idle events for team activity tracking |

**Don't:** Manually inject session info, pending tasks, or file tracking that hooks already provide.

## Compact Instructions

When summarizing this conversation, always preserve:
- File paths that were **modified** (not just read), with a one-line summary of what changed
- All decisions made during the session and their reasoning
- Active Mira goal IDs and milestone progress
- User preferences or constraints stated during the session
- The current task's specific requirements, acceptance criteria, and remaining steps
- Any errors encountered and how they were resolved (or if still unresolved)

## What NOT to Do

Beyond the anti-patterns above, avoid:
- Manually fetching session context that `UserPromptSubmit` hook already injects
- Duplicating goal/task state between Claude tasks and Mira goals
