# tools

MCP tool implementations. Contains the business logic for all user-facing tools exposed through the MCP protocol.

## Structure

Two-layer organization:
- `tools/core/` - Tool implementations (business logic)
- `tools/mod.rs` - Re-exports via `pub use core::*`

The MCP layer (`mcp/mod.rs`) calls into these functions after deserializing request parameters.

## Core Modules

| Module | Tools |
|--------|-------|
| `memory.rs` | `remember`, `recall`, `forget` |
| `code.rs` | `search_code`, `get_symbols`, `find_callers`, `find_callees`, `index`, `summarize_codebase` |
| `project.rs` | `project` (start/set/get) |
| `goals.rs` | `goal` (create/list/update/milestones) |
| `documentation.rs` | `documentation` (list/get/complete/skip/inventory/scan) |
| `reviews.rs` | `finding` (list/get/review/stats/patterns/extract) |
| `diff.rs` | `analyze_diff` |
| `experts/` | `consult_experts`, `configure_expert` |
| `session.rs` | `session_history`, `reply_to_mira` |
| `session_notes.rs` | Claude Code session note integration |
| `teams.rs` | `team` (create/invite/remove/list/members) |
| `cross_project.rs` | `cross_project` (sharing/syncing patterns) |
| `usage.rs` | `usage` (summary/stats/list) |
| `claude_local.rs` | `export_claude_local` |
| `dev.rs` | `get_session_recap` |
