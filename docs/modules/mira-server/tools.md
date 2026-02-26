<!-- docs/modules/mira-server/tools.md -->
# tools

MCP tool implementations. Contains the business logic for all user-facing tools exposed through the MCP protocol.

## Structure

Two-layer organization:
- `tools/core/` - Tool implementations (business logic)
- `mcp/router.rs` - MCP protocol layer (request routing, schema, outputSchema)

The MCP layer calls into these functions after deserializing request parameters.

## Core Modules

| Module | Tool | Actions |
|--------|------|---------|
| `code/` | `code` | `search`, `symbols`, `callers`, `callees`, `dependencies`, `patterns`, `tech_debt` |
| `code/` | `index` | `project`, `file`, `status`, `compact`, `summarize`, `health` |
| `diff.rs` | `diff` | Standalone MCP tool — semantic diff analysis |
| `project.rs` | `project` | `start`, `set`, `get` |
| `goals.rs` | `goal` | `create`, `bulk_create`, `list`, `get`, `update`, `delete`, `add_milestone`, `complete_milestone`, `delete_milestone`, `progress` |
| `documentation.rs` | `documentation` | `list`, `get`, `complete`, `skip`, `batch_skip`, `inventory`, `scan` |
| `session.rs` | `session` | `current_session`, `list_sessions`, `get_history`, `recap`, `usage_summary`, `usage_stats`, `usage_list`, `insights`, `dismiss_insight` |
| `tasks.rs` | `session` (`action="tasks_*"`) | `tasks_list`, `tasks_get`, `tasks_cancel` (fallback for clients without native MCP tasks) |
| `team.rs` | `team` | `status`, `review`, `distill` |
| `usage.rs` | — | LLM usage tracking helpers |
| `session_notes.rs` | — | Session notes helpers |
| `recipe/` | `recipe` | `list`, `get` (reusable team workflow recipes) |
