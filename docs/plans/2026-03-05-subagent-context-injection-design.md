# Subagent Context Injection Redesign

Date: 2026-03-05

## Problem

Narrow subagents (Explore, code-reviewer, code-simplifier, haiku) receive zero
context from Mira's SubagentStart hook. Goals are intentionally skipped for
narrow types, and code bundles require file paths or recognizable identifiers in
the task description -- which Explore subagents rarely have.

In practice, 5 Explore subagents launched in a real session and all got nothing.

## Design

### Approach: Hybrid project map + search hints

Two context layers, both injected for narrow subagents:

1. **Project map (always)** -- compact module/directory overview from the code
   index. Gives the subagent orientation so it knows where to look.

2. **Search hints (opportunistic)** -- run the task description through semantic
   search (embeddings with keyword fallback), inject top results as
   file:symbol pairs. Helps when it hits, silent when it doesn't.

Full subagents (Plan, general-purpose) are unchanged -- they still get goals +
code bundle.

### Project map generation

- New IPC op: `get_project_map(project_id, budget)`
- Queries `code_symbols` grouped by top-level directory
- Format: `src/commands/ (12 files), src/repository/ (8 files), ...`
- Cap at ~500 chars, sorted by file count descending
- Includes project name from `projects` table
- Latency: ~10-50ms (simple SQL aggregate)

### Search hints

- New IPC op: `search_for_subagent(project_id, query, limit)`
- Thin wrapper around existing search infrastructure
- Takes `task_description` as query, returns top 3 results as file_path:symbol pairs
- Cap at ~1000 chars
- Falls back to keyword search if embeddings unavailable
- Latency: ~100-500ms

### Output format

```
[Mira/context] Project: Conary (src/commands/, src/repository/, src/solver/, ...)

Relevant code for this task:
- src/repository/gpg.rs: verify_signature, GpgKeyring
- src/commands/install.rs: resolve_dependencies
- src/solver/mod.rs: Solver::solve
```

If search returns nothing, just the project map line.
If the project isn't indexed, empty output (same as today).

### Changes

- `subagent.rs` `run_start()`: for narrow subagents, inject project map + search
  hints instead of skipping
- New IPC client methods: `get_project_map()`, `search_for_subagent()`
- New IPC server ops to back them
- Full subagents: no change

### Latency budget

Total: ~150-550ms, well within the 5s hook timeout.
