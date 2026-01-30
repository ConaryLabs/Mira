# tools/core

Implementation layer for all MCP tool handlers. Each file contains the business logic for one or more related tools, separated from the MCP protocol layer.

## Files

| File | Implements |
|------|-----------|
| `memory.rs` | `remember`, `recall`, `forget` - semantic memory operations |
| `code.rs` | `search_code`, `get_symbols`, `find_callers`, `find_callees`, `index`, `summarize_codebase` |
| `project.rs` | `project` actions and session initialization |
| `goals.rs` | `goal` CRUD and milestone management |
| `documentation.rs` | `documentation` task lifecycle |
| `reviews.rs` | `finding` management and pattern extraction |
| `diff.rs` | `analyze_diff` semantic diff analysis |
| `experts/` | Expert consultation execution, role definitions, finding parsing |
| `session.rs` | `session_history` and `reply_to_mira` |
| `session_notes.rs` | Reading Claude Code session notes from disk |
| `teams.rs` | `team` management with authorization |
| `cross_project.rs` | Cross-project intelligence sharing |
| `usage.rs` | LLM usage analytics |
| `claude_local.rs` | CLAUDE.local.md export |
| `dev.rs` | `get_session_recap` and development utilities |

## Pattern

All tool functions follow the same signature pattern:

```rust
pub async fn tool_name<C: ToolContext>(ctx: &C, ...) -> Result<String, String>
```

The `ToolContext` trait provides access to database pools, embeddings client, LLM factory, and project context.
