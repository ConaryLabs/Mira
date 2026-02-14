<!-- docs/tools/documentation.md -->
# Documentation

> **This entire tool is CLI-only.** All actions are available via `mira tool documentation '<json>'` but are not exposed as MCP tools.

Manage documentation gap detection and writing tasks. Tracks what needs documenting, provides writing guidelines, and manages the task lifecycle.

## Actions

### list

Show documentation tasks, optionally filtered by status, type, or priority.

**Parameters:**
- `action` (string, required) - `"list"`
- `status` (string, optional) - Filter: `pending`, `completed`, `skipped`
- `doc_type` (string, optional) - Filter: `api`, `architecture`, `guide`
- `priority` (string, optional) - Filter: `urgent`, `high`, `medium`, `low`
- `limit` (integer, optional) - Max results (default: 50, max: 500)
- `offset` (integer, optional) - Pagination offset (default: 0)

**Returns:** List of tasks with status icons, IDs, categories, target paths, priorities, source paths, and reasons.

### get

Get full task details with category-specific writing guidelines.

**Parameters:**
- `action` (string, required) - `"get"`
- `task_id` (integer, required) - Task ID

**Returns:** Target path, source file, type, priority, reason, and writing guidelines tailored to the doc category (mcp_tool, module, public_api, or general).

**Note:** Only pending tasks can be retrieved. Completed or skipped tasks return an error.

### complete

Mark a documentation task as done after writing the documentation.

**Parameters:**
- `action` (string, required) - `"complete"`
- `task_id` (integer, required) - Task ID

**Returns:** Confirmation with target path.

### skip

Mark a documentation task as not needed.

**Parameters:**
- `action` (string, required) - `"skip"`
- `task_id` (integer, required) - Task ID
- `reason` (string, optional) - Reason for skipping (default: "Skipped by user")

**Returns:** Confirmation with reason.

### batch_skip

Skip multiple documentation tasks at once, either by IDs or by filter.

**Parameters:**
- `action` (string, required) - `"batch_skip"`
- `task_ids` (array of integers, optional) - Specific task IDs to skip
- `doc_type` (string, optional) - Filter matching pending tasks by type
- `priority` (string, optional) - Filter matching pending tasks by priority
- `reason` (string, optional) - Reason for skipping (default: "Batch skipped by user")

**Returns:** Count and IDs of skipped tasks, plus any errors.

**Note:** Requires either `task_ids` or at least one filter (`doc_type`/`priority`).

### inventory

Show all existing documentation with staleness indicators and impact analysis.

**Parameters:**
- `action` (string, required) - `"inventory"`

**Returns:** Documentation inventory grouped by type, with staleness warnings and change impact/summary for docs whose source files have been modified.

### scan

Trigger a fresh documentation scan of the project. Clears the scan marker so the background worker re-scans.

**Parameters:**
- `action` (string, required) - `"scan"`

**Returns:** Confirmation that the scan was triggered. Results appear in subsequent `list` calls.

## Workflow

1. `documentation(action="list", status="pending")` - See what needs docs
2. `documentation(action="get", task_id=N)` - Get source path, target path, and writing guidelines
3. Read the source file, write the documentation to the target path
4. `documentation(action="complete", task_id=N)` - Mark done

## Examples

```json
{"action": "list", "status": "pending", "priority": "high"}
```

```json
{"action": "get", "task_id": 117}
```

```json
{"action": "complete", "task_id": 117}
```

```json
{"action": "skip", "task_id": 42, "reason": "Internal function, not user-facing"}
```

```json
{"action": "batch_skip", "doc_type": "api", "reason": "Auto-generated API docs exist"}
```

```json
{"action": "inventory"}
```

```json
{"action": "scan"}
```

## Errors

- **"No active project"** - All actions require an active project context
- **"Task not found"** - The specified task ID does not exist
- **"Task belongs to a different project"** - Cross-project access denied
- **"Task is not pending"** - Only pending tasks can be completed, skipped, or retrieved via `get`
- **"task_id is required"** - `get`, `complete`, and `skip` need a task_id
- **"batch_skip requires either task_ids or a filter"** - Must provide `task_ids` or `doc_type`/`priority`

## See Also

- [project](./project.md) - Project context (required before using documentation)
- [index](./index.md) - Code indexing feeds into documentation scanning
- [code](./code.md) - Inspect file structure via `symbols` when writing docs
