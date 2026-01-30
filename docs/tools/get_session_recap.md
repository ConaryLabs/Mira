# get_session_recap

Get a session recap including preferences, recent context, active goals, and pending tasks. Provides a quick summary of where things stand at the start of a session.

## Usage

```json
{
  "name": "get_session_recap",
  "arguments": {}
}
```

## Parameters

None. Automatically uses the current project context.

## Returns

A formatted recap with up to seven sections (all optional except the welcome header):

1. **Welcome header** - Project name in a box
2. **Last chat time** - How long since the last interaction
3. **Recent sessions** - Up to 2 recent sessions with timestamps and summaries
4. **Pending tasks** - Up to 3 pending tasks with priorities
5. **Active goals** - Up to 3 active goals with progress percentages
6. **Recent insights** - Up to 3 pondering insights from the last 7 days
7. **Claude Code session notes** - Up to 3 recent session notes from Claude Code's local storage

Example output:

```
╔══════════════════════════════════════╗
║   Welcome back to Mira project!      ║
╚══════════════════════════════════════╝

Recent sessions:
• [906d58d8] 2025-01-28 05:49 - Refactored tool consolidation
• [4666c1cb] 2025-01-27 22:43 - Fixed embedding batch size

Pending tasks:
• [ ] Add unit tests for context/semantic.rs (medium)
• [ ] Phase 5: mira init setup wizard (low)

Active goals:
• SQLite concurrency improvements (75%) - in_progress
```

If no recap data is available, returns: `No session recap available.`

## Examples

**Example 1: Get recap at session start**
```json
{
  "name": "get_session_recap",
  "arguments": {}
}
```

## Errors

- **Database errors**: Failed to query session, task, or goal tables.
- **No active project**: Returns a generic welcome without project-specific context.

## See Also

- **project**: Initialize project context (provides more detailed codebase map)
- **session_history**: View detailed session history and tool call logs
- **goal**: Manage cross-session goals shown in the recap
- **recall**: Search specific memories by topic
