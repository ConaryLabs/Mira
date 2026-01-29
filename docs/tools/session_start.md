# session_start

Initialize a session with project context and persistent memory.

## Usage

```json
{
  "name": "session_start",
  "arguments": {
    "project_path": "/path/to/project",
    "name": "My Project",
    "session_id": "optional-custom-session-id"
  }
}
```

## Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| project_path | string | Yes | Absolute path to the project root directory |
| name | string | No | Display name for the project (auto-detected from Cargo.toml, package.json, or directory name) |
| session_id | string | No | Custom session ID (default: auto-generated UUID, or uses Claude Code's session ID if available) |

## Returns

A comprehensive session initialization report including:
- Project information (name, type, ID)
- Imported entries from CLAUDE.local.md (if present)
- "What's New" briefing from previous sessions
- Recent session history (last 3 sessions)
- User preferences and recent context
- Health alerts
- Documentation task notifications
- Codebase map (for Rust projects)
- Database location
- "Ready." status indicator

## Examples

**Example 1: Basic session start**
```json
{
  "name": "session_start",
  "arguments": {
    "project_path": "/home/user/projects/my-app"
  }
}
```
*Output:*
```
Project: my-app (rust)
Database: /home/user/.mira/mira.db

Ready.
```

**Example 2: Session start with custom name and session ID**
```json
{
  "name": "session_start",
  "arguments": {
    "project_path": "/home/user/projects/api-server",
    "name": "API Server v2",
    "session_id": "api-refactor-2024"
  }
}
```
*Output:*
```
Project: API Server v2 (node)
Imported 15 entries from CLAUDE.local.md

What's new: Fixed authentication bug in login endpoint

Recent sessions:
  [a1b2c3d4] 2024-01-25 14:30 - 12 tool calls (search_code, remember, get_symbols)
  [e5f6g7h8] 2024-01-24 09:15 - Refactored database layer
  Use session_history(action="get_history", session_id="...") to view details

Preferences:
  [general] Prefer async/await over callbacks
  [style] Use Result<T, E> for error handling

Recent context:
  - Working on user authentication middleware
  - Need to add rate limiting to API endpoints

Documentation: 3 items need docs
  Use `documentation(action="list")` to see them

Database: /home/user/.mira/mira.db

Ready.
```

**Example 3: Session start with existing CLAUDE.local.md**
```json
{
  "name": "session_start",
  "arguments": {
    "project_path": "/home/user/projects/legacy-system",
    "name": "Legacy Migration"
  }
}
```
*Output:*
```
Project: Legacy Migration (python)
Imported 42 entries from CLAUDE.local.md

Recent sessions:
  [x9y8z7w6] 2024-01-23 16:45 - Analyzed database schema
  [v5u4t3s2] 2024-01-22 11:20 - 8 tool calls (recall, search_code, find_callers)

Health alerts:
  [complexity] Function `process_data` has high cyclomatic complexity (15)
  [unused] Function `old_validation` appears to have no callers

Database: /home/user/.mira/mira.db

Ready.
```

## Errors

- **Invalid project path**: Returns error if the specified path doesn't exist or isn't accessible
- **Database connection failure**: Returns error if unable to connect to or initialize the Mira database
- **File system permissions**: Returns error if unable to read project files or CLAUDE.local.md
- **Session ID conflict**: Returns error if custom session ID conflicts with existing session (rare)

## See Also

- `set_project`: Set active project without full session initialization
- `get_project`: Get information about the current project
- `session_history`: Query session history and tool call logs
- `get_session_recap`: Get current session preferences and context
- `export_claude_local`: Export memories to CLAUDE.local.md for persistence