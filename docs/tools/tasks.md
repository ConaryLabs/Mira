# tasks (Background Task Management)

Long-running MCP tools like `index(action="project")` and `code(action="diff")` auto-enqueue background tasks and return a `task_id`.

## Polling (MCP clients)

Task state is **in-memory** on the running MCP server. MCP clients poll using the native protocol methods:

- **`tasks/get_info`** — check task status (working / completed / failed / cancelled)
- **`tasks/get_result`** — retrieve the finished result

These operate on the same server process that created the task, so state is always consistent.

## CLI fallback

The `session` tool's `tasks_list`, `tasks_get`, and `tasks_cancel` actions are available via CLI:

```bash
mira tool session '{"action":"tasks_list"}'
mira tool session '{"action":"tasks_get","task_id":"abc123"}'
mira tool session '{"action":"tasks_cancel","task_id":"abc123"}'
```

> **Caveat:** `mira tool` spawns a fresh server process, so it **cannot** see tasks created by the MCP server. CLI task actions are only useful for tasks created within the same CLI invocation.

## Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| action | String | Yes | `tasks_list`, `tasks_get`, or `tasks_cancel` |
| task_id | String | For get/cancel | Task ID to query or cancel |

## See Also

- [**session**](./session.md): Parent tool for task management actions
- [**index**](./index.md): Indexing operations that create background tasks
