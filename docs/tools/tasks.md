# tasks (Background Task Management)

> **Note:** Task management is accessed via the `session` tool with `tasks_*` actions.

Manage async long-running operations. Long-running tools like `index(action="project")`, `index(action="health")`, and `code(action="diff")` can enqueue background tasks. Use these actions to monitor their progress.

## Usage

```json
{
  "name": "session",
  "arguments": {
    "action": "tasks_list"
  }
}
```

## Actions

### `tasks_list` — Show all tasks

Lists running and recently completed tasks with status.

```json
{ "action": "tasks_list" }
```

### `tasks_get` — Get task status

```json
{ "action": "tasks_get", "task_id": "abc123" }
```

Returns: Task status, progress, and result (if completed).

### `tasks_cancel` — Cancel a running task

```json
{ "action": "tasks_cancel", "task_id": "abc123" }
```

## Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| action | String | Yes | `tasks_list`, `tasks_get`, or `tasks_cancel` |
| task_id | String | For get/cancel | Task ID to query or cancel |

## See Also

- [**session**](./session.md): Parent tool for task management actions
- [**index**](./index.md): Indexing operations that create background tasks
