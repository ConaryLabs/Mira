<!-- docs/tools/session.md -->
# Session

Session management, analytics, and background task tracking.

> **MCP actions:** `current_session`, `recap`
> All other actions below are **CLI-only** — use `mira tool session '<json>'`.

## Actions

### current_session

Show the current session ID.

**Parameters:**
- `action` (string, required) - `"current_session"`

**Returns:** Current session ID or "No active session".

### list_sessions (CLI-only)

List recent sessions for the active project.

**Parameters:**
- `action` (string, required) - `"list_sessions"`
- `limit` (integer, optional) - Max results (default: 20)

**Returns:** Session list with IDs, timestamps, status, summaries, and source info (startup vs resume).

### get_history (CLI-only)

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

### usage_summary (CLI-only)

Get aggregate LLM usage totals (requests, tokens, cost, average duration).

**Parameters:**
- `action` (string, required) - `"usage_summary"`
- `since_days` (integer, optional) - Look back period in days (default: 30)

**Returns:** Formatted summary with total requests, tokens, estimated cost, and average duration.

### usage_stats (CLI-only)

Get LLM usage statistics grouped by a dimension.

**Parameters:**
- `action` (string, required) - `"usage_stats"`
- `group_by` (string, optional) - Grouping dimension: `role`, `provider`, `model`, or `provider_model` (default: `role`)
- `since_days` (integer, optional) - Look back period in days (default: 30)

**Returns:** Table of usage per group with requests, tokens, and cost.

### usage_list (CLI-only)

List recent LLM usage records grouped by role.

**Parameters:**
- `action` (string, required) - `"usage_list"`
- `since_days` (integer, optional) - Look back period in days (default: 30)
- `limit` (integer, optional) - Max results (default: 50)

**Returns:** List of usage records per role.

### tasks_list (CLI-only)

List all running and recently completed background tasks.

**Parameters:**
- `action` (string, required) - `"tasks_list"`

**Returns:** Task summaries with IDs, tool names, and status (working/completed/failed/cancelled). Completed results are cached for 5 minutes.

### tasks_get (CLI-only)

Get the status and result of a specific background task.

**Parameters:**
- `action` (string, required) - `"tasks_get"`
- `task_id` (string, required) - Task ID to query

**Returns:** Task status, result text, and structured content (if completed).

### tasks_cancel (CLI-only)

Cancel a running background task.

**Parameters:**
- `action` (string, required) - `"tasks_cancel"`
- `task_id` (string, required) - Task ID to cancel

**Returns:** Confirmation or "not found" message.

### storage_status (CLI-only)

Show database storage size and data retention policy.

**Parameters:**
- `action` (string, required) - `"storage_status"`

**Returns:** Database file sizes, row counts per table, and configured retention periods.

### cleanup (CLI-only)

Run data cleanup to remove old records based on retention policy.

**Parameters:**
- `action` (string, required) - `"cleanup"`
- `category` (string, optional) - Category to clean: `sessions`, `analytics`, `chat`, `behavior`, `all` (default: `all`)
- `dry_run` (boolean, optional) - Preview what would be cleaned without deleting (default: `true`)

**Returns:** Summary of rows that would be (or were) deleted per table.

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
{"action": "usage_stats", "group_by": "provider_model", "since_days": 7}
```

```json
{"action": "tasks_get", "task_id": "abc-123"}
```

## Errors

- **"No active project"** - `list_sessions` requires an active project
- **"No active session"** - `get_history` with no session_id and no active session
- **"task_id is required"** - `tasks_get` and `tasks_cancel` need a task_id
- **"Task not found"** - The specified task ID does not exist or has expired from cache

## See Also

- [project](./project.md) - Initialize session with project context
- [memory](./memory.md) - Memories shown in recap
- [insights](./insights.md) - Background analysis digest (split from session in v0.8.0)
