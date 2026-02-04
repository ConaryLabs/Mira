# tools/core

Implementation layer for all MCP tool handlers. Each file contains the business logic for one or more related tools, separated from the MCP protocol layer.

## Files

| File | Implements |
|------|-----------|
| `memory.rs` | `memory` — remember, recall, forget |
| `code.rs` | `code` — search, symbols, callers, callees, dependencies, patterns, tech_debt; `index` — project, file, status, compact, summarize, health |
| `project.rs` | `project` — start, set, get |
| `goals.rs` | `goal` — create, bulk_create, list, get, update, delete, add_milestone, complete_milestone, delete_milestone, progress |
| `documentation.rs` | `documentation` — list, get, complete, skip, inventory, scan, export_claude_local |
| `reviews.rs` | `finding` — list, get, review, stats, patterns, extract |
| `diff.rs` | `analyze_diff` — semantic diff analysis |
| `experts/` | `expert` — consult, configure (includes council mode, role definitions, finding parsing) |
| `session.rs` | `session` — history, recap, usage, insights; `reply_to_mira` |
| `tasks.rs` | `tasks` — list, get, cancel (async background operations) |
| `teams.rs` | Team management (internal, not exposed as MCP tool) |
| `cross_project.rs` | Cross-project patterns (internal, not exposed as MCP tool) |
| `claude_local.rs` | CLAUDE.local.md export (called via `documentation(action="export_claude_local")`) |

## Pattern

Tool functions take a `ToolContext` and return structured JSON responses (via `Json<...>` output types in the MCP layer). The `ToolContext` trait provides access to database pools, embeddings client, LLM factory, and project context.
