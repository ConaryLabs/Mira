# tasks (Background Task Management)

Long-running MCP tools like `index(action="project")` and `diff(...)` auto-enqueue background tasks and return a `task_id`.

## Polling (MCP clients)

Task state is **in-memory** on the running MCP server. MCP clients poll using the native protocol methods:

- **`tasks/get`** — check task status (working / completed / failed / cancelled)
- **`tasks/result`** — retrieve the finished result
- **`tasks/cancel`** — cancel a running task

These operate on the same server process that created the task, so state is always consistent.

## See Also

- [**session**](./session.md): Session management
- [**index**](./index.md): Indexing operations that create background tasks
