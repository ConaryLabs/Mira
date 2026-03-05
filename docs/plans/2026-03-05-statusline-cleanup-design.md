# Status Line Redesign and Recipe Cleanup

Date: 2026-03-05
Status: Approved

## Problem

1. The recipe system was deleted and replaced by `launch` tool, but stale references
   remain in README, skills, and settings.

2. The status line shows basic counts (goals, indexed, alerts, pending) with a rainbow
   "Mira" prefix. It doesn't reflect Mira's actual value -- the context injections,
   subagent assistance, and hint delivery that happen behind the scenes.

## Design

### Recipe Cleanup

Remove stale references to the deleted recipe system:
- `plugin/README.md` -- remove recipe from MCP tools table
- Skill files (qa-hardening, full-cycle, refactor, experts) -- update any remaining
  recipe mentions to reference `launch` tool
- Settings -- remove `mcp__plugin_mira_mira__recipe` permission entries

### Status Line Redesign

**New format:**
```
Mira . 8 assists . 89% subagent ctx . 3 goals . 1204 indexed
```

**Display rules:**
- "Mira" prefix in dim (not rainbow)
- Segments shown conditionally (only when > 0)
- Green for positive signals (assists, subagent ctx, goals)
- Yellow for attention items (pending, alerts)
- Dim for informational (indexed)

**Segments (in order):**

| Segment | Source | Scope |
|---------|--------|-------|
| `N assists` | Non-deduped injection count | Session first, all-time fallback |
| `N% subagent ctx` | subagent_context_loads / subagent_total | Session first, all-time fallback |
| `N goals` | Active goals | Project-scoped |
| `N indexed` | Distinct indexed files | Project-scoped |
| `N pending` | Pending embeddings | Project-scoped, only if > 0 |
| `N alerts` | High-priority behavior patterns | Project-scoped, only if > 0 |

**Session detection:** Query `server_state` for `active_session_id`. If found, scope
injection queries to that session. Otherwise fall back to project-wide cumulative stats.

**Database queries:** All against existing tables with existing indexes. No schema changes.
Target: < 50ms total.

## Scope

**Building:**
- Recipe cleanup: ~3-5 files, stale reference removal
- Status line: rewrite `statusline.rs` display logic, add injection queries

**Not building:**
- No IPC integration
- No real-time push updates
- No schema changes
- No new tools or hooks

Estimated: ~6 files changed, ~100 lines modified.
