# Mira Observability and Value Visibility

Date: 2026-03-04
Status: Approved

## Problem

Mira works silently through hooks and context injection. The user has no visibility
into whether it's doing anything useful. Hook injections go into Claude's system
prompts invisibly. There's no way to tell if Mira is helping or just overhead.

## Design

Three layers of visibility, built on existing infrastructure.

### Layer 1: Real-time feedback (stderr + context tag)

Every hook that injects context emits a one-liner to stderr:

```
[Mira] SessionStart: injected 3 items (412 chars) -- goals, previous session, modified files
[Mira] PreToolUse: file hint for db/tasks.rs (reread advisory)
[Mira] UserPromptSubmit: 2 items (285 chars) -- pending tasks, convention hint
[Mira] SubagentStart: pre-loaded 3 goals (186 chars)
```

Format: `[Mira] {hook}: {summary}` -- one line, always.

Additionally, append a compact `[Mira/activity]` tag to injected context:

```
[Mira/activity] Injected: goals (2), file hints (1), conventions (1) | 342 chars | session total: 8 injections
```

This appears in the conversation transcript and helps Claude understand what context
it received.

### Layer 2: Injection content logging

Schema change -- add two columns to `context_injections`:

```sql
ALTER TABLE context_injections ADD COLUMN content TEXT;
ALTER TABLE context_injections ADD COLUMN categories TEXT;
```

- `content`: the actual injected text, truncated to ~2000 chars
- `categories`: comma-separated list (e.g. `goals,file_hints,conventions`)

Storage impact: ~2KB per injection x ~1200 injections = ~2.4MB. Negligible.
Retention policy already cleans up old injections.

### Layer 3: Enhanced `/mira:status` dashboard

Extend `session(action="status")` to show:

```
Mira Status -- Mira project

This session:
  Context delivered: 8 injections (2.1 KB)
  Deduped (suppressed): 2
  Sources: goals (3), file hints (2), conventions (2), subagent context (1)
  Files hinted: tasks.rs, session.rs, mod.rs

All time (704 sessions):
  Context delivered: 1,114 injections (333 KB)
  Subagents assisted: 437 context pre-loads
  Goals tracked: 143 across 6 projects
  Insights generated: 307
  Tool calls tracked: 2,797

Value signals:
  Stale file re-reads prevented: ~12 this session
  Subagent context hits: 89% had pre-loaded context
  Convention hints delivered: 47 this session
```

Numbers framed as actions, not raw stats. Value signals are approximate (`~` prefix).

### Layer 4: Value heuristics (outcome-based)

Three initial heuristics from `context_injections` + `tool_history` correlation:

1. **Stale file re-read correlation** -- PreToolUse hints "file X modified" then
   check if Read for that file appears within next 3 tool calls.

2. **Subagent context utilization** -- SubagentStart pre-loads context, check if
   subagent's tool_history includes related activity.

3. **Goal awareness** -- goal injection followed by goal tool calls in the session.

All computed lazily on `/mira:status` request. No background processing.

Future: once injection content is stored, parse for specific file paths/goal IDs
and correlate with subsequent tool calls for per-injection precision.

## Scope

**Building:**
- Schema migration: add `content` and `categories` to `context_injections`
- 4 hook changes (SessionStart, UserPromptSubmit, PreToolUse, SubagentStart):
  stderr summary, categories tracking, content capture, activity tag
- Dashboard extension in `session(action="status")`
- 3 correlation queries for value heuristics

**Not building:**
- No new hooks or tools
- No real-time per-injection feedback loop (future iteration)
- No UI beyond stderr + context tags + dashboard
- No MCP protocol changes
- No background processing for heuristics

## Estimated size

~6 files changed, ~300 lines added.
