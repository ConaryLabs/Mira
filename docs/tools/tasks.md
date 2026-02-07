# tasks (fallback)

> **Note:** This is not a standalone MCP tool. Access task management via `session(action="tasks", ...)`.

Manage async long-running operations. This is a fallback for clients without native MCP task support.

Long-running tools like `index(action="project")`, `index(action="health")`, and `code(action="diff")` can enqueue background tasks. Use this tool to monitor their progress.

## Usage

```json
{
  "name": "session",
  "arguments": {
    "action": "tasks",
    "tasks_action": "list"
  }
}
```

## Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| action | String | Yes | Must be `tasks` |
| tasks_action | String | Yes | `list`, `get`, or `cancel` |
| task_id | String | For get/cancel | Task ID to query or cancel |

## Actions

### `list` — Show all tasks

Lists running and recently completed tasks with status.

```json
{ "action": "tasks", "tasks_action": "list" }
```

### `get` — Get task status

```json
{ "action": "tasks", "tasks_action": "get", "task_id": "abc123" }
```

Returns: Task status, progress, and result (if completed).

### `cancel` — Cancel a running task

```json
{ "action": "tasks", "tasks_action": "cancel", "task_id": "abc123" }
```

## See Also

- [**session**](./session.md): Task fallback lives under `session(action="tasks")`
- [**index**](./index.md): Indexing operations that create background tasks
