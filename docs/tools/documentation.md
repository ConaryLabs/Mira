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
| action | String | Yes | Action to perform: `list`, `get`, `complete`, `skip`, `inventory`, or `scan` |
| task_id | Integer | Conditional | Task ID (required for `get`, `complete`, `skip`) |
| reason | String | No | Reason for skipping (used with `skip` action, defaults to "Skipped by user") |
| doc_type | String | No | Filter by documentation type (used with `list` action, e.g. `mcp_tool`, `module`, `public_api`) |
| priority | String | No | Filter by priority (used with `list` action, e.g. `high`, `medium`, `low`) |
| status | String | No | Filter by status (used with `list` action, e.g. `pending`, `applied`, `skipped`) |

### Actions

| Action | Description | Required Params |
|--------|-------------|-----------------|
| `list` | Show documentation tasks, optionally filtered | `action` |
| `get` | Get full task details with writing guidelines for a specific task | `action`, `task_id` |
| `complete` | Mark a task as done after writing the documentation | `action`, `task_id` |
| `skip` | Mark a task as not needed | `action`, `task_id` |
| `inventory` | Show all existing documentation with staleness indicators | `action` |
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

### `inventory`

Markdown inventory of all existing documentation grouped by type, with staleness warnings for docs whose source files have changed since the doc was last updated.

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
- **"Task {id} is not pending"**: Only pending tasks can be completed or skipped.
- **"task_id is required for action '{action}'"**: The `get`, `complete`, and `skip` actions require a `task_id`.

## See Also

- **project**: Initialize project context (required before using documentation)
- **index**: Index project code, which feeds into documentation scanning
- **get_symbols**: Inspect file structure when writing module documentation
