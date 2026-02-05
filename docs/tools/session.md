# session

Session management. Actions: `history` (session/tool logs), `recap` (quick overview), `usage` (LLM analytics), `insights` (unified digest), `tasks` (async task management fallback).

## Usage

```json
{
  "name": "session",
  "arguments": {
    "action": "recap"
  }
}
```

## Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| action | String | Yes | `history`, `recap`, `usage`, `insights`, or `tasks` |
| history_action | String | For history | `current`, `list_sessions`, or `get_history` |
| usage_action | String | For usage | `summary`, `stats`, or `list` |
| tasks_action | String | For tasks | `list`, `get`, or `cancel` |
| session_id | String | No | Session ID for `get_history` |
| task_id | String | For tasks | Task ID for `get` or `cancel` |
| group_by | String | No | For usage stats: `role`, `provider`, `model`, `provider_model` |
| since_days | Integer | No | Filter to last N days (default: 30) |
| limit | Integer | No | Max results |
| insight_source | String | No | Filter insights: `pondering`, `proactive`, `doc_gap` |
| min_confidence | Float | No | Min confidence for insights (default: 0.3) |

## Actions

### `history` — Query session history

Uses `history_action` sub-parameter:

**`current`** — Show current session ID:
```json
{ "action": "history", "history_action": "current" }
```

**`list_sessions`** — List recent sessions:
```json
{ "action": "history", "history_action": "list_sessions", "limit": 5 }
```

**`get_history`** — View tool calls for a session:
```json
{ "action": "history", "history_action": "get_history", "session_id": "a1b2c3d4-..." }
```

### `recap` — Session overview

Returns a formatted recap with recent sessions, active goals, pending tasks, insights, and preferences. No parameters needed.

```json
{ "action": "recap" }
```

### `usage` — LLM usage analytics

Uses `usage_action` sub-parameter:

**`summary`** — Aggregate totals:
```json
{ "action": "usage", "usage_action": "summary", "since_days": 7 }
```

**`stats`** — Grouped statistics:
```json
{ "action": "usage", "usage_action": "stats", "group_by": "provider_model" }
```

**`list`** — Recent usage records:
```json
{ "action": "usage", "usage_action": "list", "limit": 100 }
```

### `insights` — Unified insights digest

Merges pondering insights, proactive suggestions, and documentation gaps into a single queryable surface.

```json
{ "action": "insights", "insight_source": "pondering", "min_confidence": 0.5 }
```

### `tasks` — Async task management (fallback)

Use when the client does not support MCP native tasks. Uses the `tasks_action` sub-parameter.

**`list`** — Show running and completed tasks:
```json
{ "action": "tasks", "tasks_action": "list" }
```

**`get`** — Get a specific task result:
```json
{ "action": "tasks", "tasks_action": "get", "task_id": "abc123" }
```

**`cancel`** — Cancel a running task:
```json
{ "action": "tasks", "tasks_action": "cancel", "task_id": "abc123" }
```

## Errors

- **Invalid action**: Must be `history`, `recap`, `usage`, `insights`, or `tasks`
- **No active session**: No session has been started yet
- **No active project**: Some actions require an active project context

## See Also

- [**project**](./project.md): Initialize session with project context
- [**goal**](./goal.md): Goals shown in recap
- [**memory**](./memory.md): Memories shown in recap
