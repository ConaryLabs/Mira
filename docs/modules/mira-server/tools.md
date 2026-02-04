# tools

MCP tool implementations. Contains the business logic for all user-facing tools exposed through the MCP protocol.

## Structure

Two-layer organization:
- `tools/core/` - Tool implementations (business logic)
- `tools/mcp.rs` - MCP protocol layer (request routing, schema, outputSchema)

The MCP layer calls into these functions after deserializing request parameters.

## Core Modules

| Module | Tool | Actions |
|--------|------|---------|
| `memory.rs` | `memory` | `remember`, `recall`, `forget` |
| `code.rs` | `code` | `search`, `symbols`, `callers`, `callees`, `dependencies`, `patterns`, `tech_debt` |
| `code.rs` | `index` | `project`, `file`, `status`, `compact`, `summarize`, `health` |
| `project.rs` | `project` | `start`, `set`, `get` |
| `goals.rs` | `goal` | `create`, `bulk_create`, `list`, `get`, `update`, `delete`, `add_milestone`, `complete_milestone`, `delete_milestone`, `progress` |
| `documentation.rs` | `documentation` | `list`, `get`, `complete`, `skip`, `inventory`, `scan`, `export_claude_local` |
| `reviews.rs` | `finding` | `list`, `get`, `review`, `stats`, `patterns`, `extract` |
| `diff.rs` | `analyze_diff` | (standalone) |
| `experts/` | `expert` | `consult`, `configure` |
| `session.rs` | `session` | `history`, `recap`, `usage`, `insights` |
| `session.rs` | `reply_to_mira` | (standalone) |
| `tasks.rs` | `tasks` | `list`, `get`, `cancel` |
| `teams.rs` | `team` | `create`, `invite`, `remove`, `list`, `members` |
| `cross_project.rs` | `cross_project` | `get_preferences`, `enable_sharing`, `disable_sharing`, `reset_budget`, `get_stats`, `extract_patterns`, `sync` |
