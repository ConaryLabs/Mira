# tasks

Manage async long-running operations. Actions: `list` (show tasks), `get` (task status), `cancel` (stop a task).

Long-running tools like `index(action="project")` and `index(action="health")` automatically enqueue as background tasks. Use this tool to monitor their progress.

## Usage

```json
{
  "name": "tasks",
  "arguments": {
    "action": "list"
  }
}
```

## Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| action | String | Yes | `list`, `get`, or `cancel` |
| task_id | String | For get/cancel | Task ID to query or cancel |

## Actions

### `list` — Show all tasks

Lists running and recently completed tasks with status.

```json
{ "action": "list" }
```

### `get` — Get task status

```json
{ "action": "get", "task_id": "abc123" }
```

Returns: Task status, progress, and result (if completed).

### `cancel` — Cancel a running task

```json
{ "action": "cancel", "task_id": "abc123" }
```

## See Also

- [**index**](./index.md): Indexing operations that create background tasks
