# CLAUDE.md

This project uses **Mira** - a persistent memory and intelligence layer via MCP.

## Session Start

Call these at the start of every session:
```
get_guidelines(category="persona")
get_guidelines(category="mira_usage")
get_session_context()
```

All Mira documentation and usage guidance is stored in the database, not this file.

## Permission Persistence

When the user approves a tool permission, call `save_permission()` to remember it:
```
save_permission(tool_name="Bash", input_field="command", input_pattern="cargo ", match_type="prefix")
```

This enables auto-approval in future sessions via the PermissionRequest hook.

## Development

```bash
SQLX_OFFLINE=true cargo build --release
DATABASE_URL="sqlite://data/mira.db" ./target/release/mira
```
