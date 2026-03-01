# Mira Core Concepts

Mira is an intelligent "second brain" for Claude Code, designed to persist context, understand code structure, and automate development workflows. This guide explains the core concepts that power Mira.

---

## 1. Observations System

> **Note:** The Memory System described here was removed in v0.9.1. Persistent facts are now stored via the observations system (`system_observations` table), which records structured observations from hooks and tool usage. Session recap still surfaces relevant context automatically.

The legacy Memory System stored facts, decisions, and context as `MemoryFact` records with confidence scores, scopes (`project`, `personal`, `team`), and evidence-based promotion. That model has been replaced by the observations system, which is lighter-weight and managed automatically by hooks rather than requiring explicit `memory(action="store")` calls.

---

## 2. Code Intelligence

Mira doesn't just read files; it understands code structure.

### Symbol Extraction

Mira uses **Tree-sitter** to parse source code into an Abstract Syntax Tree (AST). It extracts:

- **Symbols**: Functions, structs, enums, classes, interfaces
- **Signatures**: Normalized function signatures to track API changes
- **Ranges**: Line numbers for precise navigation

Supported languages: Rust, Python, TypeScript, JavaScript, Go

> Note: Java projects are detected (via pom.xml/build.gradle) but not yet supported for code intelligence.

### Symbol Notation

Symbols use compact notation in output and context injection: `function_name:function(42)` means a function named `function_name` starting at line 42. The general format is `name:kind(line)`.

### Call Graph

Mira builds a relational graph of function calls:

| Query | What it answers |
|-------|-----------------|
| `code(action="callers", function_name="foo")` | Who calls function `foo`? |
| `code(action="callees", function_name="foo")` | What does function `foo` call? |

This allows tracing execution paths and understanding dependencies without reading every file.

### Semantic Search

Code chunks and memories are embedded into vector space using OpenAI's text-embedding-3-small model. This enables **semantic search** — finding code by meaning rather than exact keywords.

```
"authentication middleware" → finds auth-related code
"error handling"           → finds try/catch, Result types, etc.
```

### Keyword Search

The keyword search system uses FTS5 with a code-aware tokenizer (`unicode61` with underscore as a token character, no stemming). Multi-term queries use an **AND-first** strategy — all terms must match — with OR fallback if AND yields nothing.

Three strategies run in parallel:
1. **FTS5 full-text search** — AND-first with OR fallback, proximity boost for nearby terms
2. **Symbol name matching** — Scored by match quality (exact, substring, partial)
3. **LIKE chunk search** — Supplements sparse results

**Tree-guided scope narrowing**: Query terms are scored against the cartographer module tree (names, purposes, exports). Results in the top 3 matching modules receive a 1.3x score boost.

### Hybrid Search

The `code(action="search")` tool runs semantic and keyword searches in parallel, merges and deduplicates results, then applies intent-based reranking (documentation, implementation, example, or general queries get different boost profiles).

---

## 3. Intelligence Engine

The Intelligence Engine is a background worker system that proactively analyzes the codebase.

### Background Tasks

| Task | What It Does |
|------|--------------|
| **Module Summaries** | Generates human-readable descriptions of code modules |
| **Git Briefings** | "What changed since your last session?" summaries |
| **Code Health** | Scans for complexity issues, poor error handling, unused code |
| **Embeddings** | Indexes code and memories for semantic search |
| **Pondering** | Active reasoning loops that analyze tool history and generate insights |
| **Documentation** | Detects documentation gaps and stale docs |
| **Diff Outcomes** | Tracks whether predicted risks from diffs materialized |
| **Entity Extraction** | Extracts entities from memories and code for cross-referencing |
| **Proactive Items** | Generates proactive suggestions based on session context |
| **Team Monitor** | Monitors team activity in Agent Teams sessions |
| **Data Retention** | Cleans up old records to keep the database lean |

These tasks run asynchronously during idle time, keeping Mira always up-to-date.

### How It Works

```
File Change Detected → Watcher queues update
                            ↓
                    Background Worker processes
                            ↓
                    Index/Embeddings updated
                            ↓
                    Ready for next query
```

---

## 4. Sessions

A **Session** represents a continuous period of work with Claude Code.

### What's Tracked

| Data | Purpose |
|------|---------|
| **Start/End Time** | Session boundaries |
| **Tool History** | Every tool call and its result |
| **Active Project** | Which project you're working on |

### Session Recap

When you return to a project, `session(action="recap")` provides:

- Recent context from past sessions
- Pending tasks and active goals
- Git changes since last visit (briefing)
- Your stored preferences

### Evidence for Memories

Sessions serve as the "evidence" unit for the Memory System. Memories are only promoted to "confirmed" if accessed across multiple distinct sessions.

---

## 5. Session Hooks

Mira integrates with Claude Code via **hooks** that trigger at key moments during a session.

### Available Hooks

| Hook | When It Runs | Purpose |
|------|--------------|---------|
| **SessionStart** | When session begins | Captures session ID, initializes tracking |
| **UserPromptSubmit** | When user submits a prompt | Injects proactive context automatically |
| **PreToolUse** | Before Grep/Glob/Read execution | Injects relevant code context and suggests semantic alternatives |
| **PostToolUse** | After file mutations (`Write\|Edit\|NotebookEdit\|Bash`) | Tracks behavior for pattern mining |
| **PreCompact** | Before context compaction | Preserves important context before summarization |
| **Stop** | When session ends | Saves session state, auto-exports memories to CLAUDE.local.md, checks goal progress |
| **SessionEnd** | On user interrupt | Snapshots tasks for continuity |
| **SubagentStart** | When subagent spawns | Injects relevant context for subagent tasks |
| **SubagentStop** | When subagent completes | Captures discoveries from subagent work |
| **PermissionRequest** | On permission check | Auto-approve tools based on stored rules |
| **PostToolUseFailure** | After tool failure | Tracks failures, recalls relevant memories after repeated failures |
| **TaskCompleted** | When task completes | Logs completions, auto-completes matching goal milestones |
| **TeammateIdle** | When teammate goes idle | Logs idle events for team activity tracking |

### Auto-Configuration

Hooks are automatically configured by the installer in `~/.claude/settings.json`. No manual setup required.

### What Hooks Enable

- **Session tracking**: Links tool history and memories to sessions
- **Proactive context**: Automatically surfaces relevant memories and suggestions
- **Behavior learning**: Mines patterns from tool usage for future predictions
- **Context preservation**: Extracts decisions before Claude Code compacts context

---

## 6. Background Analysis

> **Note:** The Proactive Intelligence system described here was removed in v0.9.1. The `proactive` insight source no longer exists. Background analysis continues via the **pondering** system, which analyzes tool history and generates insights surfaced through `insights(action="insights")` with `insight_source="pondering"`.

The legacy Proactive Intelligence system tracked behavior patterns (file sequences, tool chains, query patterns) and generated pre-computed suggestions injected by the `UserPromptSubmit` hook. This has been replaced by:

- **Pondering**: Active reasoning loops that analyze tool history and generate insights on-demand
- **Doc gap detection**: Background scanning for missing or stale documentation (`insight_source="doc_gap"`)
- **Automatic context injection**: The `UserPromptSubmit` hook still performs semantic search and injects relevant context, but no longer uses pre-generated proactive suggestions

---

## 7. Documentation System

Mira actively manages project documentation to keep it in sync with code.

### Gap Detection

Mira automatically scans for missing documentation:

| What | Where |
|------|-------|
| MCP Tools | Functions in `mcp/router.rs` with `#[tool]` |
| Public APIs | Public types and functions in `lib.rs` |
| Modules | Core architectural modules |

### Staleness Tracking

Documentation is tracked against the code it describes:

- **Git History**: If code changes significantly, doc is flagged
- **Source Signatures**: Hash of normalized signatures detects API changes

### Generation Workflow

Documentation management is available via CLI (`mira tool documentation '<json>'`):

```
documentation(action="list")               → See what needs documentation
documentation(action="get", task_id=42)    → Get task details + guidelines
documentation(action="complete", task_id=42)  → Mark done after Claude writes
documentation(action="skip", task_id=42)   → Mark as not needed
```

Claude Code reads the source and writes documentation directly.

---

## 8. Goals and Milestones

Mira provides persistent goal tracking that survives across sessions. For in-session task tracking, use Claude Code's native task system.

### Goals

High-level objectives that span multiple sessions:
- Title and description
- Priority (low, medium, high, critical)
- Status (planning, in_progress, blocked, completed, abandoned)

```
goal(action="create", title="v2.0 Release", description="Ship new features")
goal(action="list")
goal(action="update", goal_id=1, status="in_progress")
```

### Milestones

Quantifiable steps toward a goal with weighted progress:
- **Title**: What needs to be done
- **Weight**: Impact on goal progress (default: 1, higher = more significant)
- **Status**: Completed or pending

```
goal(action="add_milestone", goal_id=1, milestone_title="Design API", weight=2)
goal(action="add_milestone", goal_id=1, milestone_title="Implement endpoints", weight=5)
goal(action="add_milestone", goal_id=1, milestone_title="Write tests", weight=3)
goal(action="complete_milestone", milestone_id=1)  # Auto-updates goal progress
goal(action="update", goal_id=1, progress_percent=75)  # Manual progress override
```

Progress is calculated from weighted milestones: completing a weight-5 milestone contributes more than a weight-1 milestone.

---

## Putting It Together

Here's how the concepts connect:

```
┌─────────────────────────────────────────────────────────────┐
│                        Session                               │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐  │
│  │   Memory    │  │    Code     │  │    Intelligence     │  │
│  │   System    │←→│ Intelligence│←→│      Engine         │  │
│  │             │  │             │  │                     │  │
│  │ - Facts     │  │ - Symbols   │  │ - Background tasks  │  │
│  │ - Evidence  │  │ - Call graph│  │ - Embeddings        │  │
│  │ - Scopes    │  │ - Search    │  │ - Summaries         │  │
│  └─────────────┘  └─────────────┘  └─────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
```

The Intelligence Engine continuously updates Code Intelligence and Memory. Sessions tie everything together with provenance and history.

---

## Local Data Storage

All data Mira collects is stored locally in `~/.mira/`. Nothing leaves your machine unless you explicitly configure an external API (e.g., OpenAI for embeddings).

### What Mira Stores

| Data | Where | Purpose |
|------|-------|---------|
| **User prompt text** | `session_behavior_log` | Pattern mining and proactive context |
| **Tool calls with arguments and result summaries** | `tool_history` | Session recall, pondering, behavior analysis |
| **File access patterns** | `session_behavior_log`, `team_file_ownership` | Workflow pattern detection, team conflict detection |
| **Query embeddings** | `vec_memory`, `vec_code` (vector tables in `mira.db` / `mira-code.db`) | Semantic search |
| **Mined behavior patterns** | `behavior_patterns` | Proactive suggestions |
| **Session summaries and snapshots** | `sessions`, `session_snapshots` | Session resume and recap |

### Security Considerations

- The `~/.mira/` directory is created with `0700` permissions (owner-only access)
- Database files use `0600` permissions (owner read/write only)
- Memory storage (`memory_facts`) applies secret detection — content that looks like API keys, tokens, or passwords is rejected
- `tool_history` does **not** apply secret detection — if Claude reads a file containing credentials, that content may end up in `tool_history.result_summary`. Treat `~/.mira/mira.db` as a sensitive file
- Project `.env` files are never loaded (prevents malicious repos from overriding API keys)

---

## MCP Resources

Mira exposes read-only data via the MCP Resource protocol. These are data access points usable by any MCP-compatible client (not just Claude Code).

| Resource URI | Type | Description |
|--------------|------|-------------|
| `mira://goals` | Static | List of all active goals with progress percentages |
| `mira://goals/{id}` | Template | Individual goal with its milestones |

Resources are read-only and scoped to the active project. They complement the `goal` tool — use resources for passive data display (e.g., in a dashboard or sidebar) and tools for interactive operations.
