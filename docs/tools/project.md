<!-- docs/tools/project.md -->
# Project

Manage project context and workspace initialization.

> **MCP actions:** `start`, `get`
> Actions marked (CLI-only) below are available via `mira tool project '<json>'`.

## Actions

### start

Initialize a session with full project context. Detects project type, imports CLAUDE.local.md memories, loads preferences, recent sessions, health alerts, and generates a codebase map (Rust projects).

**Parameters:**
- `action` (string, required) - `"start"`
- `project_path` (string, required) - Absolute path to the project root
- `name` (string, optional) - Project name override (auto-detected from Cargo.toml/package.json if omitted)
- `session_id` (string, optional) - Session ID (falls back to Claude's hook-generated ID, then UUID)

**Returns:** Project ID, name, type, codebase map, recent sessions, preferences, health alerts, pending documentation count, and database path.

### get

Show the currently active project.

**Parameters:**
- `action` (string, required) - `"get"`

**Returns:** Project ID, name, and path. Returns an error message if no project is active.

## Auto-Detection

| File | Detected Type | Name Source |
|------|--------------|-------------|
| `Cargo.toml` | rust | `[package] name` (workspace falls back to directory name) |
| `package.json` | node | `"name"` field |
| `pyproject.toml` / `setup.py` | python | directory name |
| `go.mod` | go | directory name |
| `pom.xml` / `build.gradle` | java | directory name |
| *(none)* | unknown | directory name |

## Examples

```json
{"action": "start", "project_path": "/home/user/my-project"}
```

```json
{"action": "get"}
```

## Errors

- **"project_path is required"** - The `start` action needs a `project_path`.
- **"No active project"** - The `get` action returns this when no project is initialized.

## Notes

- The `start` action is typically called automatically by Mira's session hooks. Manual use is rarely needed.
- Side effects: creates/updates project and session records, imports CLAUDE.local.md, stores system context, registers file watcher, generates codebase map.
- Project context is required by most other tools (memory, code, documentation, etc.).

## See Also

- [session](./session.md) - Session management and recap
- [index](./index.md) - Codebase indexing (uses module summaries from project start)
- [memory](./memory.md) - Memories scoped to project
