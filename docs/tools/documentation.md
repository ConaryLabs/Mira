# documentation

Manage documentation tasks. Tracks what needs documenting across the project, provides writing guidelines, and manages task lifecycle from pending through completion or skip.

## Usage

```json
{
  "name": "documentation",
  "arguments": {
    "action": "list",
    "status": "pending"
  }
}
```

## Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| action | String | Yes | Action to perform: `list`, `get`, `complete`, `skip`, `batch_skip`, `inventory`, or `scan` |
| task_id | Integer | Conditional | Task ID (required for `get`, `complete`, `skip`) |
| task_ids | Array[Integer] | No | List of task IDs (used with `batch_skip`) |
| reason | String | No | Reason for skipping (used with `skip`/`batch_skip`, defaults to "Skipped by user") |
| doc_type | String | No | Filter by documentation type: `api`, `architecture`, `guide` |
| priority | String | No | Filter by priority: `urgent`, `high`, `medium`, `low` |
| status | String | No | Filter by status: `pending`, `completed`, `skipped` |
| limit | Integer | No | Max results for `list` (default: 50, max: 500) |
| offset | Integer | No | Offset for `list` pagination (default: 0) |

### Actions

| Action | Description | Required Params |
|--------|-------------|-----------------|
| `list` | Show documentation tasks, optionally filtered | `action` |
| `get` | Get full task details with writing guidelines for a specific task | `action`, `task_id` |
| `complete` | Mark a task as done after writing the documentation | `action`, `task_id` |
| `skip` | Mark a task as not needed | `action`, `task_id` |
| `batch_skip` | Skip multiple tasks at once | `action`, plus `task_ids` or filters |
| `inventory` | Show all existing documentation with staleness indicators and impact data | `action` |
| `scan` | Trigger a fresh documentation scan of the project | `action` |

## Returns

### `list`

Markdown-formatted list of documentation tasks with status icons, IDs, priorities, source paths, and reasons.

### `get`

Full task details including target path, source file, type, priority, and category-specific writing guidelines (parameter tables for tools, architecture notes for modules, etc.).

### `complete`

Confirmation message: `Task {id} marked complete. Documentation written to {path}.`

### `skip`

Confirmation message: `Task {id} skipped: {reason}`

### `batch_skip`

Summary of skipped tasks with IDs and any errors for tasks that couldn't be skipped.

### `inventory`

Markdown inventory of all existing documentation grouped by type (stable alphabetical ordering), with staleness warnings and impact analysis (`change_impact`, `change_summary`) for docs whose source files have changed.

### `scan`

Confirmation that the scan was triggered. Results appear in subsequent `list` calls.

## Examples

**Example 1: List all pending high-priority tasks**
```json
{
  "name": "documentation",
  "arguments": {
    "action": "list",
    "status": "pending",
    "priority": "high"
  }
}
```

**Example 2: Get task details and writing guidelines**
```json
{
  "name": "documentation",
  "arguments": {
    "action": "get",
    "task_id": 117
  }
}
```

**Example 3: Skip a task that doesn't need documentation**
```json
{
  "name": "documentation",
  "arguments": {
    "action": "skip",
    "task_id": 42,
    "reason": "Internal function, not user-facing"
  }
}
```

## Workflow

1. `documentation(action="list", status="pending")` - See what needs docs
2. `documentation(action="get", task_id=N)` - Get the source path, target path, and writing guidelines
3. Read the source file, write the documentation to the target path
4. `documentation(action="complete", task_id=N)` - Mark done

## Errors

- **"No active project"**: Requires an active project context.
- **"Task {id} not found"**: The specified task ID does not exist.
- **"Task {id} belongs to a different project"**: The task is associated with another project.
- **"Task {id} is not pending (status: {status}). Cannot skip."**: Only pending tasks can be completed or skipped.
- **"task_id is required for action '{action}'"**: The `get`, `complete`, and `skip` actions require a `task_id`.
- **"batch_skip requires either task_ids or a filter"**: The `batch_skip` action needs `task_ids` or `doc_type`/`priority` filters.

## See Also

- **project**: Initialize project context (required before using documentation)
- **index**: Index project code, which feeds into documentation scanning
- [**code**](./code.md): Inspect file structure via `code(action="symbols")` when writing docs
