# CLAUDE.md

Mira is a Rust MCP server providing persistent memory and code intelligence for Claude Code.

## Session Start

Project context is **auto-initialized** from Claude Code's working directory.
For full session context, call `session(action="recap")`. Use `memory(action="recall", query="preferences")` before writing code.

**Automatic bridging:** Mira hooks capture session source (`startup` vs `resume`), pending tasks, and working directory. Session history shows `[startup]` or `[resume←previous_id]`.

**Notation:** `tool(action="x", param="y")` refers to MCP tool calls. For example, `memory(action="recall", query="...")` calls the `memory` MCP tool with `action="recall"`. These are not shell commands.

## Anti-Patterns

**NEVER** do these in the Mira codebase:

| Don't | Do Instead |
|-------|------------|
| Use `Database` directly | Use `DatabasePool` for all database access |
| Store secrets in memories | Keep secrets in `.env` only |
| Guess at MCP tool parameters | Check tool schema or existing usage first |
| Add dependencies without checking | Run `memory(action="recall", query="dependencies")` first |
| Change tool handler signatures in `mcp/router.rs` | Coordinate changes across all tool modules in `tools/core/` |

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
cargo build
cargo test
```

The binary is at `target/debug/mira` (or `target/release/mira` for release builds). Claude Code spawns it via MCP (configured in `.mcp.json`).

## Debugging

```bash
mira debug-session   # Debug project(action="start") output
mira debug-carto     # Debug cartographer module detection
```

## Environment

API keys are in `~/.mira/.env` (optional with MCP Sampling):
- `DEEPSEEK_API_KEY` - Background LLM tasks (pondering, summaries)
- `OPENAI_API_KEY` - Embeddings (OpenAI text-embedding-3-small)
- `OLLAMA_HOST` - Local LLM for background tasks (no API key needed)

Optional: `OLLAMA_MODEL`, `BRAVE_API_KEY` (web search), `DEFAULT_LLM_PROVIDER`, `MIRA_DISABLE_LLM`, `MIRA_USER_ID`.
See `.env.example` for all options.

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
| `/mira:recall [query]` | Browse or search stored memories |
| `/mira:insights` | Surface background analysis and predictions |
| `/mira:diff` | Semantic analysis of code changes |
| `/mira:experts` | Expert consultation via Agent Teams |
| `/mira:full-cycle` | End-to-end expert review with implementation and QA |
| `/mira:qa-hardening` | Production readiness review and hardening backlog |
| `/mira:refactor` | Safe code restructuring with architect + reviewer validation |
| `/mira:help` | Show all available Mira commands and tools |
| `/mira:efficiency` | Token efficiency stats and active optimization features |
| `/mira:status` | Quick health check: index stats, storage, active goals |

## Hook Integration

Mira hooks **automatically inject context** — don't manually duplicate this:

| Hook | What It Injects |
|------|-----------------|
| `SessionStart` | Session ID, startup vs resume, task list ID, working directory; on resume: previous session context, goals, team info, incomplete tasks from previous session |
| `UserPromptSubmit` | Pending tasks, relevant memories, proactive predictions, pre-generated suggestions, team context |
| `PreToolUse` | Relevant memories before Grep/Glob/Read (semantic search with keyword fallback); file reread advisory and symbol hints for Read |
| `PostToolUse` | Tracks file modifications, team conflict detection |
| `PreCompact` | Extracts decisions, TODOs, and errors from transcript before summarization |
| `Stop` | Session snapshot, task export, goal progress check, auto-export to CLAUDE.local.md |
| `SessionEnd` | Snapshot tasks on user interrupt, team session cleanup |
| `SubagentStart` | Injects relevant memories and active goals for subagent context |
| `SubagentStop` | Captures discoveries from subagent work |
| `PermissionRequest` | Auto-approve tools based on stored rules |
| `PostToolUseFailure` | Tracks tool failures, recalls relevant memories after 3+ repeated failures |
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
- Memory IDs stored via `memory(action="remember")` during this session

## What NOT to Do

Beyond the anti-patterns above, avoid:
- Manually fetching session context that `UserPromptSubmit` hook already injects
- Creating memories for ephemeral info (hooks track file access automatically)
- Duplicating goal/task state between Claude tasks and Mira goals
