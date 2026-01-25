# Mira Core Concepts

Mira is an intelligent "second brain" for Claude Code, designed to persist context, understand code structure, and automate development workflows. This guide explains the core concepts that power Mira.

---

## 1. Memory System

The Memory System is the foundation of Mira's persistence. It stores facts, decisions, and context that outlive a single chat session.

### Memory Fact

The basic unit of storage is a `MemoryFact`. Each fact has:

| Field | Description |
|-------|-------------|
| **Content** | The textual information (e.g., "The project uses Postgres 14") |
| **Fact Type** | Categorizes the memory (see below) |
| **Confidence** | A score (0.0 - 1.0) indicating reliability |
| **Scope** | Where the memory applies (project, personal, team) |
| **Status** | Lifecycle state: `candidate`, `verified`, or `rejected` |
| **Category** | Optional grouping (e.g., "coding", "architecture") |
| **Owner** | User ID or Team ID for shared/scoped memories |

### Fact Types

| Type | Purpose |
|------|---------|
| `general` | Standard facts about the codebase or project |
| `preference` | User preferences (e.g., "Use async-trait for traits") |
| `decision` | Architectural or design decisions |
| `context` | Background information about the project |
| `health` | Code health issues detected by scanners |
| `capability` | Discovered features or tools in the codebase |

### Evidence-Based Confidence

Memories follow a lifecycle based on evidence:

```
New Memory → Candidate (max 0.5 confidence)
                ↓
        Used across 3+ sessions
                ↓
         Confirmed (boosted confidence)
```

1. **Candidate**: New memories start here with capped confidence
2. **Confirmed**: If a memory is accessed across 3+ distinct sessions, it's promoted

This ensures only useful, recurring information becomes permanent.

### Scopes

| Scope | Visibility |
|-------|------------|
| `project` | Only visible within the current project (default) |
| `personal` | Visible across all your projects |
| `team` | Shared with team members |

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
| `find_callers("foo")` | Who calls function `foo`? |
| `find_callees("foo")` | What does function `foo` call? |

This allows tracing execution paths and understanding dependencies without reading every file.

### Semantic Search

Code chunks and memories are embedded into vector space using Google or OpenAI embeddings. This enables **semantic search** - finding code by meaning rather than exact keywords.

```
"authentication middleware" → finds auth-related code
"error handling"           → finds try/catch, Result types, etc.
```

### Capability Detection

Mira proactively discovers what your codebase can do:

```
check_capability("caching")     → "Found Redis caching in src/cache/"
check_capability("auth")        → "JWT auth in src/middleware/auth.rs"
```

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
| **Documentation Writer** | Generates comprehensive documentation |

### Provider Configuration

Experts can be backed by different LLM providers via `configure_expert` or `~/.mira/config.toml`:
- **DeepSeek** (default) - Optimized for extended reasoning
- **Gemini** - Google's models, cost-effective
- **GLM** - Z.AI's GLM 4.7 with thinking mode

Each expert role can use a different provider based on the task requirements.

### How Experts Work

Experts operate in a multi-turn **agentic loop**:

```
1. Reason  → Analyze the request, decide what info is needed
2. Act     → Call tools (search_code, read_file, find_callers...)
3. Observe → Tool output feeds back into context
4. Iterate → Continue until task complete (max 100 iterations)
```

### Tool Access

Experts can use these tools to explore the codebase:

- `search_code` - Semantic code search
- `read_file` - Read file contents
- `get_symbols` - Get functions/classes in a file
- `find_callers` / `find_callees` - Trace call relationships
- `recall` - Search memories

### Learned Patterns

For Code Reviewer and Security roles, Mira injects "Previously Identified Patterns" from memory. If you correct an issue (e.g., "Always validate inputs"), the expert remembers in future sessions.

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

When you return to a project, `session_start` provides:

- Recent context from past sessions
- Pending tasks and active goals
- Git changes since last visit (briefing)
- Your stored preferences

### Evidence for Memories

Sessions serve as the "evidence" unit for the Memory System. Memories are only promoted to "confirmed" if accessed across multiple distinct sessions.

---

## 6. Documentation System

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
list_doc_tasks()         → See what needs documentation
write_documentation(42)  → Expert generates and writes the doc
skip_doc_task(42)        → Mark as not needed
```

The Documentation Writer expert explores the actual implementation to produce accurate, comprehensive docs.

---

## 7. Goals and Milestones

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
│  │                    Expert System                         │ │
│  │  Architect | Code Reviewer | Security | Doc Writer | ... │ │
│  └─────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
```

The Intelligence Engine continuously updates Code Intelligence and Memory. Experts can query both to provide informed analysis. Sessions tie everything together with provenance and history.
