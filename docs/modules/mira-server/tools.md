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
| `memory.rs` | `memory` | `remember`, `recall`, `forget`, `archive`, `export_claude_local` |
| `code/` | `code` | `search`, `symbols`, `callers`, `callees`, `dependencies`, `patterns`, `tech_debt`, `diff` |
| `code/` | `index` | `project`, `file`, `status`, `compact`, `summarize`, `health` |
| `project.rs` | `project` | `start`, `set`, `get` |
| `goals.rs` | `goal` | `create`, `bulk_create`, `list`, `get`, `update`, `delete`, `add_milestone`, `complete_milestone`, `delete_milestone`, `progress` |
| `documentation.rs` | `documentation` | `list`, `get`, `complete`, `skip`, `batch_skip`, `inventory`, `scan` |
| `diff.rs` | `code` (`action="diff"`) | Semantic diff analysis |
| `session.rs` | `session` | `history`, `recap`, `usage`, `insights` |
| `session.rs` | `reply_to_mira` | (standalone) |
| `tasks.rs` | `session` (`action="tasks"`) | `list`, `get`, `cancel` (fallback for clients without native MCP tasks) |
| `team.rs` | `team` | `status`, `review`, `distill` |
| `usage.rs` | — | LLM usage tracking helpers |
| `session_notes.rs` | — | Session notes helpers |
| `recipe.rs` | `recipe` | `list`, `get` (reusable team workflow recipes) |
