# session

Session management, analytics, and background task tracking. All actions are flat — no nested sub-actions.

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
| action | String | Yes | One of: `current_session`, `list_sessions`, `get_history`, `recap`, `usage_summary`, `usage_stats`, `usage_list`, `insights`, `dismiss_insight`, `tasks_list`, `tasks_get`, `tasks_cancel` |
| session_id | String | No | Session ID for `get_history` |
| task_id | String | For tasks | Task ID for `tasks_get` or `tasks_cancel` |
| group_by | String | No | For `usage_stats`: `role`, `provider`, `model`, `provider_model` |
| since_days | Integer | No | Filter to last N days (default: 30) |
| limit | Integer | No | Max results |
| insight_source | String | No | Filter insights: `pondering`, `proactive`, `doc_gap` |
| min_confidence | Float | No | Min confidence for insights (default: 0.3) |
| insight_id | Integer | For dismiss_insight | Insight row ID to dismiss |

## Actions

### `current_session` — Show current session ID

```json
{ "action": "current_session" }
```

### `list_sessions` — List recent sessions

```json
{ "action": "list_sessions", "limit": 5 }
```

### `get_history` — View tool calls for a session

```json
{ "action": "get_history", "session_id": "a1b2c3d4-..." }
```

### `recap` — Session overview

Returns a formatted recap with recent sessions, active goals, pending tasks, insights, and preferences.

```json
{ "action": "recap" }
```

### `usage_summary` — Aggregate LLM usage totals

```json
{ "action": "usage_summary", "since_days": 7 }
```

### `usage_stats` — Grouped LLM usage statistics

```json
{ "action": "usage_stats", "group_by": "provider_model" }
```

### `usage_list` — Recent LLM usage records

```json
{ "action": "usage_list", "limit": 100 }
```

### `insights` — Unified insights digest

Merges pondering insights, proactive suggestions, and documentation gaps into a single queryable surface.

```json
{ "action": "insights", "insight_source": "pondering", "min_confidence": 0.5 }
```

### `dismiss_insight` — Remove a resolved insight

```json
{ "action": "dismiss_insight", "insight_id": 42 }
```

### `tasks_list` — Show running and completed tasks

```json
{ "action": "tasks_list" }
```

### `tasks_get` — Get a specific task result

```json
{ "action": "tasks_get", "task_id": "abc123" }
```

### `tasks_cancel` — Cancel a running task

```json
{ "action": "tasks_cancel", "task_id": "abc123" }
```

## Errors

- **Invalid action**: Must be one of the 12 supported actions
- **No active session**: No session has been started yet
- **No active project**: Some actions require an active project context

## See Also

- [**project**](./project.md): Initialize session with project context
- [**goal**](./goal.md): Goals shown in recap
- [**memory**](./memory.md): Memories shown in recap
