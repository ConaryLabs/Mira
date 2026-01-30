# session_history

Query session history. List recent sessions, view tool call history for a specific session, or check the current session ID.

## Usage

```json
{
  "name": "session_history",
  "arguments": {
    "action": "list_sessions",
    "limit": 10
  }
}
```

## Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| action | String | Yes | Action to perform: `current`, `list_sessions`, or `get_history` |
| session_id | String | No | Session ID for `get_history` (falls back to current session if omitted) |
| limit | Integer | No | Max results (default: 20) |

### Actions

| Action | Description | Required Params |
|--------|-------------|-----------------|
| `current` | Show the current active session ID | `action` |
| `list_sessions` | List recent sessions with status and tool call counts | `action` |
| `get_history` | Show tool call history for a session | `action` |

## Returns

### `current`

```
Current session: a1b2c3d4-5678-90ab-cdef-1234567890ab
```

Or: `No active session`

### `list_sessions`

```
3 sessions:
  [a1b2c3d4] 2026-01-29 10:23 - active (15 tool calls)
  [e5f6a7b8] 2026-01-28 14:10 - completed (42 tool calls)
  [c9d0e1f2] 2026-01-27 09:30 - completed (8 tool calls)
```

Or: `No sessions found.`

### `get_history`

```
5 tool calls in session a1b2c3d4:
  ✓ remember [2026-01-29 10:24] Stored preference about...
  ✓ recall [2026-01-29 10:25] Found 3 matching memories...
  ✗ search_code [2026-01-29 10:26] Index not available...
```

Or: `No history for session a1b2c3d4`

## Examples

**Example 1: Check current session**
```json
{
  "name": "session_history",
  "arguments": { "action": "current" }
}
```

**Example 2: List recent sessions**
```json
{
  "name": "session_history",
  "arguments": { "action": "list_sessions", "limit": 5 }
}
```

**Example 3: View tool calls from a specific session**
```json
{
  "name": "session_history",
  "arguments": {
    "action": "get_history",
    "session_id": "a1b2c3d4-5678-90ab-cdef-1234567890ab"
  }
}
```

## Errors

- **"No active session"**: No session has been started yet.
- **"No active project"**: `list_sessions` requires an active project context.
- **Database errors**: Failed to query session or tool history tables.

## See Also

- **get_session_recap**: Quick overview of recent activity, goals, and preferences
- **project**: Initialize a session with `action: "start"`
