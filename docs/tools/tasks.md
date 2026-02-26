<!-- docs/tools/tasks.md -->
# tasks (Background Task Management)

Fallback tool for clients without native MCP task support. Manages background tasks created by long-running operations like `index(action="project")` and `diff(...)`.

> **CLI-only** -- use `mira tool tasks '<json>'`.
> This tool is not exposed via the MCP tool router. MCP clients should use the native protocol methods (`tasks/get`, `tasks/result`, `tasks/cancel`) instead.

## Actions

### list

List all running and recently completed background tasks.

**Parameters:**
- `action` (string, required) - `"list"`

**Returns:** Task summaries with IDs, tool names, and status (`working`/`completed`/`failed`/`cancelled`). Completed results are cached for 5 minutes.

### get

Get the status and result of a specific background task.

**Parameters:**
- `action` (string, required) - `"get"`
- `task_id` (string, required) - Task ID to query

**Returns:** Task status, result text, and structured content (if completed).

### cancel

Cancel a running background task.

**Parameters:**
- `action` (string, required) - `"cancel"`
- `task_id` (string, required) - Task ID to cancel

**Returns:** Confirmation message, or "not found" if the task does not exist or already completed.

## MCP Native Polling

MCP clients that support the native task protocol should use these methods instead of the `tasks` tool:

- **`tasks/get`** -- check task status (working / completed / failed / cancelled)
- **`tasks/result`** -- retrieve the finished result
- **`tasks/cancel`** -- cancel a running task

These operate on the same server process that created the task, so state is always consistent.

## Examples

```json
{"action": "list"}
```

```json
{"action": "get", "task_id": "abc-123"}
```

```json
{"action": "cancel", "task_id": "abc-123"}
```

## Errors

- **"task_id is required"** -- `get` and `cancel` need a `task_id`
- **"Task not found"** -- The specified task ID does not exist, has expired from cache, or already completed

## See Also

- [**index**](./index.md) -- Indexing operations that create background tasks
- [**diff**](./diff.md) -- Diff analysis that may run as a background task
