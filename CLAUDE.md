# CLAUDE.md

This project uses **Mira** - a persistent memory and intelligence layer via MCP.

## Session Start

Call once at the start of every session:
```
session_start(project_path="/home/peter/Mira")
```

This single call sets the project, loads persona, corrections, goals, tasks, and recent context.

All Mira documentation and usage guidance is stored in the database, not this file.

## Permission Persistence

When the user approves a tool permission, call `save_permission()` to remember it:
```
save_permission(tool_name="Bash", input_field="command", input_pattern="cargo ", match_type="prefix")
```

This enables auto-approval in future sessions via the PermissionRequest hook.
