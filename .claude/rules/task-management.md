# Task and Goal Management

## Session Tasks (Claude Built-in)

For current session work, use Claude Code's native task system:
- `TaskCreate` - Create tasks for multi-step work
- `TaskUpdate` - Mark in_progress/completed, set dependencies
- `TaskList` - View current session tasks

These are session-scoped and optimized for real-time workflow tracking.

**Mira integration (automatic):**
- Pending tasks are injected as context on each prompt (via UserPromptSubmit hook)
- Tasks are snapshotted to Mira's database when the session ends (via Stop hook)
- Session resume is tracked (`startup` vs `resume`) in session history

## Cross-Session Goals (Mira)

For work spanning multiple sessions, use Mira's `goal` tool with milestones:

```
goal(action="create", title="Implement auth system", priority="high")
goal(action="add_milestone", goal_id="1", milestone_title="Design API", weight=2)
goal(action="complete_milestone", milestone_id="1")  # Auto-updates progress
goal(action="list")  # Shows goals with progress %
```

**When to use goals:** Multi-session objectives, tracking progress over time, breaking large work into weighted milestones.

**Goal statuses:** planning, in_progress, blocked, completed, abandoned

**Priorities:** low, medium, high, critical

**Goal actions:**
- `create` / `bulk_create` - Create new goal(s)
- `list` / `get` - View goals and their milestones
- `update` / `progress` - Update goal fields or progress
- `delete` - Remove a goal
- `add_milestone` - Add milestone with optional weight
- `complete_milestone` - Mark done (auto-updates goal progress)
- `delete_milestone` - Remove a milestone

## Quick Reference

| Need | Tool |
|------|------|
| Track work in THIS session | Claude's `TaskCreate` |
| Track work across sessions | Mira's `goal` |
| Add sub-items to goal | `goal(action="add_milestone")` |
| Check long-term progress | `goal(action="list")` |

Don't use goal tracking for trivial single-action tasks.
