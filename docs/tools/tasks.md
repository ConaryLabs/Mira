# tasks (Background Task Management)

> **Note:** Task management is CLI-only. Use `mira tool session '{"action":"tasks_list"}'` etc.
> These actions are **not** available via the MCP tool surface.

Manage async long-running operations. Long-running tools like `index(action="project")` and `code(action="diff")` can enqueue background tasks. Use these actions to monitor their progress.

## Usage (CLI only)

```bash
mira tool session '{"action":"tasks_list"}'
```

## Actions

### `tasks_list` — Show all tasks

Lists running and recently completed tasks with status.

```bash
mira tool session '{"action":"tasks_list"}'
```

### `tasks_get` — Get task status

```bash
mira tool session '{"action":"tasks_get","task_id":"abc123"}'
```

Returns: Task status, progress, and result (if completed).

### `tasks_cancel` — Cancel a running task

```bash
mira tool session '{"action":"tasks_cancel","task_id":"abc123"}'
```

## Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| action | String | Yes | `tasks_list`, `tasks_get`, or `tasks_cancel` |
| task_id | String | For get/cancel | Task ID to query or cancel |

## Important

Task state is **in-memory** on the running MCP server process. The `mira tool` CLI spawns a separate server, so it cannot see tasks from the MCP server. Task polling is only reliable within the same server process.

## See Also

- [**session**](./session.md): Parent tool for task management actions
- [**index**](./index.md): Indexing operations that create background tasks
