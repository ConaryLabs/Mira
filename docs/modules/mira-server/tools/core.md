<!-- docs/modules/mira-server/tools/core.md -->
# tools/core

Implementation layer for all MCP tool handlers. Each file contains the business logic for one or more related tools, separated from the MCP protocol layer.

## Files

| File | Implements |
|------|-----------|
| `memory.rs` | `memory` — remember, recall, forget, archive, export_claude_local |
| `code/` | `code` — search, symbols, callers, callees, dependencies, patterns, tech_debt; `index` — project, file, status, compact, summarize, health |
| `diff.rs` | `diff` — standalone MCP tool for semantic diff analysis |
| `project.rs` | `project` — start, set, get |
| `goals.rs` | `goal` — create, bulk_create, list, get, update, delete, add_milestone, complete_milestone, delete_milestone, progress |
| `documentation.rs` | `documentation` — list, get, complete, skip, batch_skip, inventory, scan |
| `session.rs` | `session` — current_session, list_sessions, get_history, recap, usage_summary, usage_stats, usage_list, insights, dismiss_insight |
| `tasks.rs` | `session` (`action="tasks_*"`) — tasks_list, tasks_get, tasks_cancel (fallback for clients without native MCP tasks) |
| `launch.rs` | `launch` — parse agent files, enrich with project context, return agent specs |
| `team.rs` | `team` — status, review, distill |
| `usage.rs` | LLM usage tracking helpers |
| `session_notes.rs` | Session notes helpers |
| `claude_local/` | CLAUDE.local.md export (called via `memory(action="export_claude_local")`) |

## Pattern

Tool functions take a `ToolContext` and return structured JSON responses (via `Json<...>` output types in the MCP layer). The `ToolContext` trait provides access to database pools, embeddings client, LLM factory, and project context.
