# project

Manage project context. Initializes sessions with a codebase map, switches between projects, and shows the current project.

## Usage

```json
{
  "name": "project",
  "arguments": {
    "action": "start",
    "project_path": "/home/user/myproject"
  }
}
```

## Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| action | String | Yes | Action to perform: `start`, `set`, or `get` |
| project_path | String | Conditional | Absolute path to project root (required for `start` and `set`) |
| name | String | No | Project name (auto-detected from Cargo.toml, package.json, or directory name if omitted) |
| session_id | String | No | Session ID for `start` action (auto-generated if omitted) |

### Actions

| Action | Description | Required Params |
|--------|-------------|-----------------|
| `start` | Initialize a session with full project context, codebase map, and insights | `action`, `project_path` |
| `set` | Switch the active project without full initialization | `action`, `project_path` |
| `get` | Show the current active project | `action` |

## Returns

### `start`

A comprehensive project initialization report containing:

- **Project header**: Name and detected type (rust, node, python, go, java)
- **CLAUDE.local.md import**: Count of memory entries imported, if the file exists (bidirectional: imported on start, auto-exported on session close via Stop hook)
- **What's new briefing**: Summary of changes since last session
- **Recent sessions**: Last few sessions with timestamps and summaries
- **Session insights**: User preferences, recent context, health alerts, proactive analysis results, and pending documentation count
- **Codebase map**: Hierarchical module structure with purposes and key exports (Rust projects)
- **Database path**: Location of the project database

Also performs side effects: creates/updates the project and session in the database, imports CLAUDE.local.md memories, stores system context, registers the file watcher, and generates the codebase map.

### `set`

Confirmation message:

```
Project set: myproject (id: 5)
```

### `get`

Current project info:

```
Current project:
  Path: /home/user/myproject
  Name: myproject
  ID: 5
```

Or if no project is active: `No active project. Call set_project first.`

## Examples

**Example 1: Initialize a session**
```json
{
  "name": "project",
  "arguments": {
    "action": "start",
    "project_path": "/home/user/myproject"
  }
}
```

**Example 2: Switch to a different project**
```json
{
  "name": "project",
  "arguments": {
    "action": "set",
    "project_path": "/home/user/other-project",
    "name": "other-project"
  }
}
```

**Example 3: Check current project**
```json
{
  "name": "project",
  "arguments": {
    "action": "get"
  }
}
```

## Auto-Detection

The `start` and `set` actions auto-detect project metadata:

| File | Detected Type | Name Source |
|------|--------------|-------------|
| `Cargo.toml` | rust | `[package] name` |
| `package.json` | node | `"name"` field |
| `pyproject.toml` | python | `[project] name` |
| `go.mod` | go | module path |
| `pom.xml` | java | artifact ID |
| *(none)* | unknown | directory name |

## Errors

- **"project_path is required for action 'start'"**: The `start` action needs a `project_path`.
- **"project_path is required for action 'set'"**: The `set` action needs a `project_path`.
- **"No active project"**: The `get` action returns this when no project is initialized.
- **Database errors**: Failed to create or query the project record.

## See Also

- **get_session_recap**: Get session recap with preferences, context, and goals
- **session_history**: View past session activity
- **index**: Index project code for semantic search and symbols
- **documentation**: Manage documentation tasks for the project
