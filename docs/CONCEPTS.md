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
| `health` | Code health issues detected by scanners |
| `capability` | Discovered features or tools in the codebase |
| `system` | Internal system markers (used internally) |

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

Code chunks and memories are embedded into vector space using Google's gemini-embedding-001 model. This enables **semantic search** — finding code by meaning rather than exact keywords.

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
| **Capability Inventory** | Discovers features, tools, APIs in the codebase |
| **Code Health** | Scans for complexity issues, poor error handling |
| **Tool Extraction** | Extracts insights from tool results into memories |
| **Embeddings** | Indexes code and memories for semantic search |

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

## 4. Expert System

Mira employs specialized "Expert" agents to handle complex analysis tasks.

### Expert Roles

| Role | Use Case |
|------|----------|
| **Architect** | System design, patterns, scalability |
| **Plan Reviewer** | Validates implementation plans before coding |
| **Scope Analyst** | Detects requirements gaps and edge cases |
| **Code Reviewer** | Bugs, safety, code quality patterns |
| **Security** | Vulnerabilities and hardening |

### Provider Configuration

Experts can be backed by different LLM providers via `expert(action="configure")` or `~/.mira/config.toml`:
- **MCP Sampling** (zero-key) — Uses the host client, no API keys needed
- **DeepSeek** (default with key) — Optimized for extended reasoning
- **Gemini** — Google's models, cost-effective for simpler tasks

Each expert role can use a different provider based on the task requirements.

### How Experts Work

#### Single Expert Mode

A single expert runs in a multi-turn **agentic loop**:

```
1. Reason  → Analyze the request, decide what info is needed
2. Act     → Call tools (code search, read_file, callers...)
3. Observe → Tool output feeds back into context
4. Iterate → Continue until task complete (max 100 iterations)
```

#### Council Mode (Multi-Expert)

When multiple experts are consulted, Mira uses a **council architecture** with a coordinator that orchestrates the consultation:

```
Plan     → Coordinator creates a research plan with tasks per expert
Execute  → Experts run assigned tasks in parallel (agentic loops)
Review   → Coordinator reviews all findings, identifies conflicts
Delta    → If conflicts exist, targeted follow-up questions (up to 2 rounds)
Synthesize → Final synthesis combining all findings
```

Each expert records structured **findings** (topic, content, evidence, severity, recommendation) via a `store_finding` tool. The coordinator reviews these findings to identify consensus, conflicts, and gaps.

If the council pipeline fails, it falls back gracefully to parallel independent consultations.

#### Reasoning Strategy

Expert consultations use a `ReasoningStrategy` to manage LLM clients:

- **Single**: One model handles both tool-calling and synthesis
- **Decoupled**: A chat model (`deepseek-chat`) handles tool loops, and a reasoning model (`deepseek-reasoner`) handles final synthesis. This split prevents OOM from unbounded `reasoning_content` accumulation during long tool loops.

### Tool Access

Experts can use these tools to explore the codebase:

- `code(action="search")` — Semantic code search
- `read_file` — Read file contents
- `code(action="symbols")` — Get functions/classes in a file
- `code(action="callers")` / `code(action="callees")` — Trace call relationships
- `memory(action="recall")` — Search memories
- `web_fetch` / `web_search` — Web access (if API keys configured)
- **MCP tools** — Tools from external MCP servers in the host environment

### Learned Patterns

For Code Reviewer and Security roles, Mira injects "Previously Identified Patterns" from the `corrections` table. If you correct a finding (e.g., "Always validate inputs"), the expert applies that pattern in future sessions.

### Expert Prompts

Expert system prompts include:
- **Stakes framing** — Context on why the review matters
- **Accountability rules** — Experts must cite evidence and avoid speculation
- **Self-checks** — Required verification steps before finalizing output

---

## 5. Sessions

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

## 6. Session Hooks

Mira integrates with Claude Code via **hooks** that trigger at key moments during a session.

### Available Hooks

| Hook | When It Runs | Purpose |
|------|--------------|---------|
| **SessionStart** | When session begins | Captures session ID, initializes tracking |
| **UserPromptSubmit** | When user submits a prompt | Injects proactive context automatically |
| **PostToolUse** | After file mutations (`Write\|Edit\|NotebookEdit`) | Tracks behavior for pattern mining |
| **PreCompact** | Before context compaction | Preserves important context before summarization |
| **Stop** | When session ends | Saves session state, auto-exports memories to CLAUDE.local.md, checks goal progress |

### Auto-Configuration

Hooks are automatically configured by the installer in `~/.claude/settings.json`. No manual setup required.

### What Hooks Enable

- **Session tracking**: Links tool history and memories to sessions
- **Proactive context**: Automatically surfaces relevant memories and suggestions
- **Behavior learning**: Mines patterns from tool usage for future predictions
- **Context preservation**: Extracts decisions before Claude Code compacts context

---

## 7. Proactive Intelligence

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

## 8. Documentation System

Mira actively manages project documentation to keep it in sync with code.

### Gap Detection

Mira automatically scans for missing documentation:

| What | Where |
|------|-------|
| MCP Tools | Functions in `mcp/mod.rs` with `#[tool]` |
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

Claude Code reads the source and writes documentation directly - no expert system overhead.

---

## 9. Goals and Milestones

Mira provides persistent goal tracking that survives across sessions. For in-session task tracking, use Claude Code's native task system.

### Goals

High-level objectives that span multiple sessions:
- Title and description
- Priority (low, medium, high, critical)
- Status (planning, in_progress, blocked, completed, abandoned)

```
goal(action="create", title="v2.0 Release", description="Ship new features")
goal(action="list")
goal(action="update", goal_id="1", status="in_progress")
```

### Milestones

Quantifiable steps toward a goal with weighted progress:
- **Title**: What needs to be done
- **Weight**: Impact on goal progress (default: 1, higher = more significant)
- **Status**: Completed or pending

```
goal(action="add_milestone", goal_id="1", milestone_title="Design API", weight=2)
goal(action="add_milestone", goal_id="1", milestone_title="Implement endpoints", weight=5)
goal(action="add_milestone", goal_id="1", milestone_title="Write tests", weight=3)
goal(action="complete_milestone", milestone_id="1")  # Auto-updates goal progress
goal(action="progress")  # Shows weighted progress percentage
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
│         ↑                ↑                    ↑              │
│         └────────────────┼────────────────────┘              │
│                          ↓                                   │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │              Expert System (Council)                     │ │
│  │  Coordinator → Architect | Code Reviewer | Security | …  │ │
│  │  FindingsStore ← structured findings from all experts    │ │
│  └─────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
```

The Intelligence Engine continuously updates Code Intelligence and Memory. Experts can query both to provide informed analysis. Sessions tie everything together with provenance and history.
