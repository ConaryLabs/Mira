# Mira Core Concepts

Mira is an intelligent "second brain" for Claude Code, designed to persist context, understand code structure, and automate development workflows. This guide explains the core concepts that power Mira.

---

## 1. Memory System

The Memory System is the foundation of Mira's persistence. It stores facts, decisions, and context that outlive a single chat session.

### Memory Fact

The basic unit of storage is a `MemoryFact`. Each fact has:

| Field | Description |
|-------|-------------|
| **content** | The textual information (e.g., "The project uses Postgres 14") |
| **fact_type** | Categorizes the memory (see below) |
| **confidence** | A score (0.0 - 1.0) indicating reliability |
| **scope** | Where the memory applies: `project`, `personal`, or `team` |
| **status** | Lifecycle state: `candidate` or `confirmed` |
| **category** | Optional grouping (e.g., "coding", "architecture") |
| **user_id** | User identity for personal-scoped memories |
| **team_id** | Team reference for team-scoped memories |
| **session_count** | Number of sessions where this memory was accessed |

### Fact Types

| Type | Purpose |
|------|---------|
| `general` | Standard facts about the codebase or project |
| `preference` | User preferences (e.g., "Use async-trait for traits") |
| `decision` | Architectural or design decisions |
| `context` | Background information about the project |

### Evidence-Based Confidence

Memories follow a lifecycle based on evidence:

```
New Memory → Candidate
                ↓
        Used across 3+ sessions
                ↓
         Confirmed (confidence + 0.2, capped at 1.0)
```

1. **Candidate**: New memories start here
2. **Confirmed**: If a memory is accessed across 3+ distinct sessions, it's promoted with boosted confidence

This ensures only useful, recurring information becomes permanent.

### Scopes

| Scope | Visibility |
|-------|------------|
| `project` | Only visible within the current project (default) |
| `personal` | Visible across all your projects (requires user identity) |
| `team` | Shared with team members (requires team membership) |

**Note:** Personal scope requires a user identity (from git config, `MIRA_USER_ID`, or system username). Team scope requires team membership.

### Branch-Aware Context

Memories are boosted based on branch relevance during recall:

| Branch Match | Boost |
|--------------|-------|
| Same branch as current | 15% priority boost (distance × 0.85) |
| Main/master branch | 5% priority boost (distance × 0.95) |
| Different branch | No boost (still accessible) |

This ensures branch-specific knowledge is prioritized while maintaining cross-branch access.

---

## 2. Code Intelligence

Mira doesn't just read files; it understands code structure.

### Symbol Extraction

Mira uses **Tree-sitter** to parse source code into an Abstract Syntax Tree (AST). It extracts:

- **Symbols**: Functions, structs, enums, classes, interfaces
- **Signatures**: Normalized function signatures to track API changes
- **Ranges**: Line numbers for precise navigation

Supported languages: Rust, Python, TypeScript, JavaScript, Go

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
| **Code Health** | Scans for complexity issues, poor error handling |
| **Embeddings** | Indexes code and memories for semantic search |
| **Pondering** | Active reasoning loops that analyze tool history and generate insights |

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
| **PostToolUse** | After file mutations (`Write\|Edit\|NotebookEdit`) | Tracks behavior for pattern mining |
| **PreCompact** | Before context compaction | Preserves important context before summarization |
| **Stop** | When session ends | Saves session state, auto-exports memories to CLAUDE.local.md, checks goal progress |
| **SessionEnd** | On user interrupt | Snapshots tasks for continuity |
| **SubagentStart** | When subagent spawns | Injects relevant context for subagent tasks |
| **SubagentStop** | When subagent completes | Captures discoveries from subagent work |
| **PermissionRequest** | On permission check | Auto-approve tools based on stored rules |

### Auto-Configuration

Hooks are automatically configured by the installer in `~/.claude/settings.json`. No manual setup required.

### What Hooks Enable

- **Session tracking**: Links tool history and memories to sessions
- **Proactive context**: Automatically surfaces relevant memories and suggestions
- **Behavior learning**: Mines patterns from tool usage for future predictions
- **Context preservation**: Extracts decisions before Claude Code compacts context

---

## 6. Proactive Intelligence

Mira proactively analyzes behavior to predict and inject helpful context before you ask.

### Behavior Tracking

The system tracks:
- **User queries**: Questions and search patterns
- **File sequences**: Common file access patterns
- **Tool chains**: Frequently used tool combinations

### Pattern Mining

Patterns are mined in two tiers:

| Tier | Method | Frequency | Purpose |
|------|--------|-----------|---------|
| **SQL Mining** | Database analysis | Every ~15 minutes | Fast, local pattern detection |
| **LLM Enhancement** | DeepSeek analysis | Every ~50 minutes | Deeper insight generation |

### Automatic Context Injection

When you submit a prompt, the `UserPromptSubmit` hook:

1. Performs semantic search for relevant memories
2. Checks for pre-generated suggestions matching your context
3. Runs on-the-fly pattern matching
4. Injects combined context into your session

This happens transparently - relevant context appears without explicit `recall()` calls.

### Proactive Suggestions

The system generates suggestions based on:
- Recurring file access patterns
- Common tool sequences
- Previously useful context retrievals

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
goal(action="progress", goal_id=1)  # Shows weighted progress percentage
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
