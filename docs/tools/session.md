<!-- docs/tools/session.md -->
# Session

Session management, analytics, and background task tracking.

## Actions

### current_session

Show the current session ID.

**Parameters:**
- `action` (string, required) - `"current_session"`

**Returns:** Current session ID or "No active session".

### list_sessions

List recent sessions for the active project.

**Parameters:**
- `action` (string, required) - `"list_sessions"`
- `limit` (integer, optional) - Max results (default: 20)

**Returns:** Session list with IDs, timestamps, status, summaries, and source info (startup vs resume).

### get_history

View tool call history for a session.

**Parameters:**
- `action` (string, required) - `"get_history"`
- `session_id` (string, optional) - Session ID to query (defaults to current session)
- `limit` (integer, optional) - Max results (default: 20)

**Returns:** Tool calls with names, timestamps, success status, and result previews.

### recap

Get a formatted session recap with preferences, recent context, active goals, pending tasks, and Claude Code session notes.

**Parameters:**
- `action` (string, required) - `"recap"`

**Returns:** Formatted text combining preferences, recent memories, and session notes.

### usage_summary

Get aggregate LLM usage totals (requests, tokens, cost, average duration).

**Parameters:**
- `action` (string, required) - `"usage_summary"`
- `since_days` (integer, optional) - Look back period in days (default: 30)

**Returns:** Formatted summary with total requests, tokens, estimated cost, and average duration.

### usage_stats

Get LLM usage statistics grouped by a dimension.

**Parameters:**
- `action` (string, required) - `"usage_stats"`
- `group_by` (string, optional) - Grouping dimension: `role`, `provider`, `model`, or `provider_model` (default: `role`)
- `since_days` (integer, optional) - Look back period in days (default: 30)

**Returns:** Table of usage per group with requests, tokens, and cost.

### usage_list

List recent LLM usage records grouped by role.

**Parameters:**
- `action` (string, required) - `"usage_list"`
- `since_days` (integer, optional) - Look back period in days (default: 30)
- `limit` (integer, optional) - Max results (default: 50)

**Returns:** List of usage records per role.

### insights

Query unified insights digest combining pondering analysis, proactive suggestions, and documentation gaps.

**Parameters:**
- `action` (string, required) - `"insights"`
- `insight_source` (string, optional) - Filter by source: `pondering`, `proactive`, `doc_gap`
- `min_confidence` (float, optional) - Minimum confidence threshold 0.0-1.0 (default: 0.5)
- `since_days` (integer, optional) - Look back period in days (default: 30)
- `limit` (integer, optional) - Max results (default: 20)

**Returns:** List of insights with priority scores, descriptions, evidence, and row IDs for dismissal.

### dismiss_insight

Remove a resolved insight so it no longer appears in future queries.

**Parameters:**
- `action` (string, required) - `"dismiss_insight"`
- `insight_id` (integer, required) - Row ID of the insight to dismiss

**Returns:** Confirmation or "not found" message.

### tasks_list

List all running and recently completed background tasks.

**Parameters:**
- `action` (string, required) - `"tasks_list"`

**Returns:** Task summaries with IDs, tool names, and status (working/completed/failed/cancelled). Completed results are cached for 5 minutes.

### tasks_get

Get the status and result of a specific background task.

**Parameters:**
- `action` (string, required) - `"tasks_get"`
- `task_id` (string, required) - Task ID to query

**Returns:** Task status, result text, and structured content (if completed).

### tasks_cancel

Cancel a running background task.

**Parameters:**
- `action` (string, required) - `"tasks_cancel"`
- `task_id` (string, required) - Task ID to cancel

**Returns:** Confirmation or "not found" message.

## Examples

```json
{"action": "recap"}
```

```json
{"action": "list_sessions", "limit": 5}
```

```json
{"action": "get_history", "session_id": "a1b2c3d4-..."}
```

```json
{"action": "insights", "insight_source": "pondering", "min_confidence": 0.7}
```

```json
{"action": "usage_stats", "group_by": "provider_model", "since_days": 7}
```

```json
{"action": "tasks_get", "task_id": "abc-123"}
```

## Errors

- **"No active project"** - `list_sessions`, `insights`, and `dismiss_insight` require an active project
- **"No active session"** - `get_history` with no session_id and no active session
- **"insight_id is required"** - `dismiss_insight` needs an insight_id
- **"task_id is required"** - `tasks_get` and `tasks_cancel` need a task_id
- **"Task not found"** - The specified task ID does not exist or has expired from cache

## See Also

- [project](./project.md) - Initialize session with project context
- [memory](./memory.md) - Memories shown in recap
