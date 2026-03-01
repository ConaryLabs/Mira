<!-- docs/DESIGN.md -->
# Mira Design Philosophy

> **Mira is a local-first "second brain for Claude Code".**
>
> It runs as an **MCP server over stdio**, stores durable memory in **SQLite
> databases** (with **sqlite-vec** for embeddings), and provides code intelligence
> plus background heuristic analysis of code patterns and session behavior.

This document explains Mira's architecture and, more importantly, the *why* behind
the major decisions. It is written for developers evaluating whether to adopt
Mira, extend it, or embed it into their own workflows.

---

## Table of Contents

- [Core Product Goals](#core-product-goals)
- [Non-Goals](#non-goals)
- [High-Level Architecture](#high-level-architecture)
- [Decision 1: MCP over HTTP API](#decision-1-mcp-over-http-api)
- [Decision 2: SQLite + sqlite-vec](#decision-2-sqlite--sqlite-vec)
- [Decision 3: Multi-Provider Intelligence](#decision-3-multi-provider-intelligence)
- [Decision 4: Evidence-Based Memory](#decision-4-evidence-based-memory)
- [Decision 5: Background Processing](#decision-5-background-processing)
- [Decision 6: Local-First by Default](#decision-6-local-first-by-default)
- [Key Subsystems](#key-subsystems)
- [Security, Privacy, and Safety](#security-privacy-and-safety)
- [Tradeoffs Summary](#tradeoffs-summary)
- [Future Direction](#future-direction)

---

## Core Product Goals

Mira is built around a few durable goals:

1. **Persistence across sessions**
   - Remember preferences, decisions, and recurring context.
   - Persist tool usage history and session identifiers for continuity.

2. **High-leverage code intelligence**
   - Semantic code search (embedding-based) with keyword fallbacks.
   - Structural understanding: symbols, call graph, imports.

3. **Ambient/continuous intelligence**
   - Background processing that keeps the "brain" warm:
     embeddings, summaries, briefings, documentation tasks, health scans.

4. **Local-first defaults**
   - One install, one binary, two database files.
   - No required cloud service, no required daemon, no required accounts.

5. **Honest, evidence-grounded memory**
   - Memories have confidence and lifecycle.
   - Mira tracks evidence (cross-session usage) and promotes candidates over time.

---

## Non-Goals

Mira intentionally does **not** attempt to be:

- A cloud SaaS memory platform (at least not by default).
- A multi-tenant server that you must deploy before it's useful.
- A general "agent framework" with unlimited autonomous execution.
- A replacement for your VCS, issue tracker, or team docs.

These non-goals keep the system small, auditable, and easy to adopt.

---

## High-Level Architecture

At runtime, Mira is a local process launched by Claude Code and spoken to using
the **Model Context Protocol (MCP)** via **stdio**.

```
Claude Code  <--stdio/MCP-->  Mira Server
                                   |
                    +--------------+--------------+
                    |              |              |
                 SQLite      Background       Embeddings
              (sqlite-vec)     Worker        (OpenAI, optional)
```

Key components:

| Component | Location | Purpose |
|-----------|----------|---------|
| MCP Server | `mcp/mod.rs`, `mcp/router.rs` | Tool router, stdio transport, outputSchema |
| Database | `db/mod.rs` | SQLite wrapper, schema, migrations |
| Background Worker | `background/mod.rs` | Embeddings, summaries, health checks |
| File Watcher | `background/watcher.rs` | Incremental indexing on file changes |
| Embeddings | `embeddings/mod.rs` | Embedding queue and OpenAI client (text-embedding-3-small, optional) |
| MCP Sampling | `llm/sampling.rs` | LLM consultation via host client (awaiting Claude Code support) |
| Elicitation | `mcp/elicitation.rs` | Interactive API key setup flow |
| Async Tasks | `tools/core/tasks.rs` | Background task management |
| Change Intelligence | `background/change_patterns.rs` | Outcome tracking, pattern mining, predictive risk |
| Entity Layer | `entities/mod.rs` | Lightweight entity extraction for recall boost |

---

## Decision 1: MCP over HTTP API

### What We Chose

Mira is an **MCP server** designed to be spawned directly by Claude Code and
communicate over **stdio**, rather than an HTTP API hosted as a separate service.

### Why This Is Right

**1) Zero deployment friction**
- No ports, no reverse proxies, no service manager.
- A "second brain" should feel like a local capability, not infrastructure.

**2) Lower security surface area**
- No network-exposed API by default.
- Avoids auth, TLS, CSRF, CORS, and secret distribution problems.

**3) Better UX for Claude Code**
- Claude Code already speaks MCP; stdio is the native local plugin shape.
- Lifecycle management is simpler: Claude spawns, uses, and exits Mira.

**4) Operational simplicity**
- Logs, state, and data live on the machine.
- Debugging is "read local files + run local binary."

### Tradeoffs

| Pro | Con |
|-----|-----|
| No network config needed | Harder to use remotely without an adapter |
| Secure by default | Single-user bias |
| Simple lifecycle | Process restarts with Claude |

### Future Evolution

The internal design keeps the door open for additional transports. The server state
already contains hooks for collaboration primitives. A future transport could wrap
the same tool router behind a local Unix socket, loopback HTTP, or secure tunnel.

---

## Decision 2: SQLite + sqlite-vec

### What We Chose

Mira stores everything in SQLite and embeds vector search using `sqlite-vec`
(`vec0` virtual tables). The main database lives at `~/.mira/mira.db`, with a
separate code index database at `~/.mira/mira-code.db` to avoid write contention
during indexing.

### Why SQLite Is Strategic

**1) Minimal-file persistence**
- Two database files are portable, inspectable, and easy to back up.
- You can move your brain between machines.

**2) Minimal dependencies**
- No Postgres, no Qdrant, no Redis required.
- Reduces "yak shave" before Mira is useful.

**3) Sane performance at Mira's scale**
- Mira's workload: small metadata reads/writes, batched inserts, vector queries.
- SQLite performs extremely well for this when tuned (WAL mode enabled).

**4) Security benefits**
- File permissions locked down (directory 0700, file 0600).
- No network listener for the database.

### Why sqlite-vec Instead of External Vector DB

**1) One system of record**
- Embeddings live beside the facts and code metadata.
- Backups and migrations remain "one thing."

**2) Lower failure modes**
- No network partitions or service dependency.
- No mismatch between relational metadata and vector index state.

### Tradeoffs

| Pro | Con |
|-----|-----|
| Local-file simplicity | Not "infinite scale" |
| Zero infrastructure | Schema evolution requires discipline |
| Unified querying | Write contention under heavy parallelism |

---

## Decision 3: Heuristic-First Intelligence

### What We Chose

Background intelligence (summaries, briefings, pondering, code health) runs entirely
on heuristic analysis. No LLM provider is required or used for background tasks.

The only optional external API is **OpenAI embeddings** (`text-embedding-3-small`)
for semantic search. Without it, search falls back to FTS5 keyword and fuzzy matching.

### Why This Architecture

**1) Zero-key usability**
- Mira works fully out of the box without any API keys.
- Heuristic analysis covers diff classification, module summaries, insight generation,
  and code health pattern detection.

**2) Predictable behavior**
- Heuristic results are deterministic and fast.
- No latency variance from external API calls in the background.

**3) Optional semantic upgrade**
- Adding an OpenAI API key upgrades search from keyword-only to hybrid semantic + keyword.
- This is the only capability gated on an API key.

### Tradeoffs

| Pro | Con |
|-----|-----|
| Works without any API keys | Semantic search requires OpenAI API key |
| Deterministic background analysis | Heuristic summaries less detailed than LLM |
| No background API costs | — |

---

## Decision 4: Evidence-Based Memory

### What We Chose

Mira treats memory as **hypotheses that earn trust over time**, not perfect facts
the moment they are written.

- New memories default to `status = 'candidate'`
- Promotion to `'confirmed'` occurs after repeated cross-session usage
- Recall records access to build evidence

### Why This Matters

**1) Prevents memory poisoning**
- Users and agents often write partial, speculative, or temporary notes.
- Evidence-based promotion reduces long-term harm from early mistakes.

**2) Matches how real teams operate**
- A decision becomes "real" when repeated across sessions and tasks.

**3) Makes memory measurable**
- Confidence becomes something the system can justify and evolve.

### User Override

Mira supports explicit user-written persistence via `CLAUDE.local.md` export.
When a human writes it into a project's local memory file, it's marked as
confirmed with high confidence.

### Tradeoffs

| Pro | Con |
|-----|-----|
| Prevents bad memories | Heuristic thresholds |
| Self-healing over time | Slower initial learning |
| Measurable confidence | Added complexity |

---

## Decision 5: Background Processing

### What We Chose

Mira includes a background worker loop that continuously processes queued work,
rather than doing everything only on-demand.

Work includes:
- Pending embeddings
- Module summaries
- Project briefings (git changes)
- Documentation gap detection
- Code health checks
- Knowledge distillation
- Pondering (active reasoning loops)

### Why Background Work Is Critical

**1) Latency matters**
- When you ask "search code by meaning," embeddings should already exist.

**2) It's the difference between a tool and a "second brain"**
- A second brain should notice drift, remember what changed, maintain indices.

**3) Enables incremental updates**
- File watcher queues changes, background pipeline processes them.

**4) Heuristic analysis keeps the brain running**
- All background tasks use heuristic analysis.
- Module summaries, diff analysis, and pondering produce useful output without any API keys.

### Tradeoffs

| Pro | Con |
|-----|-----|
| Fast interactive queries | CPU/network usage when "idle" |
| Continuously fresh data | Failure handling complexity |
| Incremental updates | Must not surprise users |
| Works without any API keys | Semantic search requires OpenAI key |

---

## Decision 6: Local-First by Default

### What We Chose

All Mira state lives locally unless you explicitly opt into external providers:
- Default DB: `~/.mira/mira.db`
- Embeddings (OpenAI) are optional and require `OPENAI_API_KEY`

### Why This Is Fundamental

**1) Trust**
- Developers are rightly cautious about shipping code context to the cloud.
- Local-first keeps the trust boundary small by default.

**2) Reliability**
- No internet required to access stored memory.
- Core functionality works offline (keyword search, memories, history).

**3) Portability**
- Local DB files are easy to backup, sync, archive, or inspect.

### Tradeoffs

| Pro | Con |
|-----|-----|
| Privacy by default | No built-in cross-device sync |
| Works offline | Local disk is single point of failure |
| Easy to inspect | Enterprise governance needs more |

---

## Key Subsystems

### MCP Server and Tools

Mira exposes **8 MCP tools** (`project`, `code`, `goal`, `index`, `session`, `insights`, `diff`, `launch`), consolidated from ~20 standalone tools in v0.4.x through action-based unification.
Tools return structured JSON via MCP `outputSchema`, enabling programmatic consumption.
The server implements MCP Elicitation (interactive setup) and MCP Tasks (async long-running operations).

This architecture encourages:
- A stable "capabilities surface" with fewer, more capable tools
- Decoupled internal implementation that can evolve

### Database Schema

The schema is "product-shaped," not purely technical:

| Family | Tables | Purpose |
|--------|--------|---------|
| Memory | `memory_facts`, `vec_memory` | Persistent memories with embeddings |
| Code | `code_symbols`, `call_graph`, `vec_code` | Code intelligence |
| Sessions | `sessions`, `tool_history` | Provenance and history |
| Background | `pending_embeddings`, `project_briefings` | Work queues |
| Workflow | `goals`, `milestones` | Goal and milestone tracking |
| Proactive | `behavior_patterns`, `proactive_suggestions` | Behavior mining and predictions |

### Embeddings and Search

Embeddings are optional (OpenAI text-embedding-3-small). Semantic search happens
in two areas:
1. **Memory recall** - `vec_memory` queried with cosine distance
2. **Code search** - Hybrid semantic + keyword search via `vec_code` and `code_fts`

### Documentation System

Mira detects documentation gaps and tracks staleness:
- Gap detection for undocumented tools, APIs, modules
- Staleness tracking when source changes
- Claude Code writes docs directly based on task details and source analysis

### Session Hooks

Mira integrates with Claude Code via hooks that trigger at key moments:

| Hook | Purpose |
|------|---------|
| `SessionStart` | Captures session ID for tracking |
| `UserPromptSubmit` | Injects proactive context into prompts |
| `PostToolUse` | Tracks behavior for pattern mining |
| `PreCompact` | Preserves context before summarization |
| `Stop` | Saves session state and checks goal progress |
| `SessionEnd` | Snapshots tasks on user interrupt |
| `SubagentStart` | Injects context when subagents spawn |
| `SubagentStop` | Captures discoveries from subagent work |
| `PreToolUse` | Injects context before tool execution (suggests semantic alternatives) |
| `PermissionRequest` | Auto-approves tools based on stored permission rules |
| `PostToolUseFailure` | Tracks tool failures, recalls memories after repeated failures |
| `TaskCompleted` | Logs task completions, auto-completes matching goal milestones |
| `TeammateIdle` | Logs teammate idle events for team activity tracking |

Hooks are auto-configured by the installer.

### Proactive Intelligence

A two-tier system that predicts and surfaces relevant context:

1. **Behavior Mining** (SQL-based, every ~15 minutes)
   - File access sequences
   - Tool usage chains
   - Query patterns

The `UserPromptSubmit` hook injects relevant suggestions automatically.

---

## Security, Privacy, and Safety

### Local Data Security

- Database directory: mode 0700
- Database file: mode 0600
- No network listener by default

### API Keys

Keys read from environment variables (`DEEPSEEK_API_KEY`, `OPENAI_API_KEY`, etc.)
and `.env` files (global `~/.mira/.env`).

### Attack Surface

By default: no network listener, no inbound socket, no HTTP server.

Main risks:
- Local machine compromise
- Accidental exfiltration through external LLM providers
- Overly broad file access during indexing

### Safety in Prompts

Safety guidelines are embedded into prompt construction via `PromptBuilder`.
Future evolution: policy-enforced safety rather than prompt-enforced.

---

## Tradeoffs Summary

| Decision | We Chose | We Gave Up |
|----------|----------|------------|
| Transport | MCP/stdio | Easy remote access |
| Storage | SQLite local files | Horizontal scaling |
| Intelligence | Heuristic-only | Embedding-powered search requires OpenAI API key |
| Memory | Evidence-based | Instant trust |
| Processing | Background worker | Zero idle resource use |
| Data | Local-first | Built-in sync |

---

## Future Direction

### Recently Implemented ✓

The following were previously planned and are now complete:
- ✓ Async database pool migration
- ✓ Session hooks for Claude Code integration
- ✓ Proactive intelligence (behavior tracking, pattern mining)
- ✓ Cross-project intelligence sharing with privacy protections
- ✓ Memory evidence with session tracking

### Recently Implemented (v0.5.0) ✓

- ✓ MCP Sampling implemented (awaiting Claude Code capability advertisement)
- ✓ MCP Elicitation for interactive API key setup
- ✓ MCP Tasks for async long-running operations
- ✓ Structured JSON responses via outputSchema
- ✓ Tool consolidation from ~20 to 8 action-based MCP tools (project, code, goal, index, session, insights, diff, launch) with documentation, memory, and team available as CLI-only
- ✓ Change Intelligence (outcome tracking, pattern mining, predictive risk)
- ✓ Entity layer for memory recall boost
- ✓ Dependency graphs, architectural pattern detection, tech debt scoring
- ✓ Context-aware convention injection

### Near-Term: Polish and Reliability

- Improve watcher/indexer reliability for large codebases
- Better conflict resolution for contradicting memories
- Enhanced pattern mining accuracy

### Medium-Term: Deeper Intelligence

- More sophisticated behavior prediction models
- Team collaboration features beyond pattern sharing

### Long-Term: Safe Autonomy

- Job queue with explicit budgets (tokens, time, cost)
- Per-provider egress controls
- Optional encrypted replication for team sharing

The thesis: Mira evolves from "memory + search" into a **local intelligence OS**
where developers expect their coding environment to maintain continuously updated
code understanding, durable project narrative, and validated memory - all while
remaining safe, inspectable, and local by default.
