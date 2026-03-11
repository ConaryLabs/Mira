# Task and Goal Management

## Session Tasks (Claude Built-in)

For current session work, use Claude Code's native task system:
- `TaskCreate` - Create tasks for multi-step work
- `TaskUpdate` - Mark in_progress/completed, set dependencies
- `TaskList` - View current session tasks

These are session-scoped and optimized for real-time workflow tracking.

**Mira integration (automatic):**
- Tasks are snapshotted to Mira's database when the session ends (via Stop hook)
- Session resume is tracked (`startup` vs `resume`) in session history

## Cross-Session Goals (Mira)

For work spanning multiple sessions, use Mira's goal functions via `run()`:

```rhai
goal_create("Implement auth system", "high")
goal_add_milestone(1, "Design API", 2)
goal_complete_milestone(1)   // Auto-updates progress
goal_list()                  // Shows goals with progress %
```

**When to use goals:** Multi-session objectives, tracking progress over time, breaking large work into weighted milestones.

**Goal statuses:** planning, in_progress, blocked, completed, abandoned

**Priorities:** low, medium, high, critical

Use `run('help()')` to list all available goal functions.

## Quick Reference

| Need | Tool |
|------|------|
| Track work in THIS session | Claude's `TaskCreate` |
| Track work across sessions | Mira's `goal` |
| Add sub-items to goal | `run('goal_add_milestone(id, "title", weight)')` |
| Check long-term progress | `run('goal_list()')` |

Don't use goal tracking for trivial single-action tasks.
