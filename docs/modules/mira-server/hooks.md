<!-- docs/modules/mira-server/hooks.md -->
# hooks

Claude Code hook handlers for lifecycle integration points.

## Overview

Hooks are invoked by Claude Code at specific events during a session. Each hook reads JSON input from stdin, performs its work (usually database operations), and writes JSON output to stdout. Hooks run as separate short-lived processes (`mira hook <action>`), not inside the MCP server process.

## Key Functions

- `read_hook_input()` -- Reads JSON from stdin (capped at 1MB)
- `write_hook_output(value)` -- Writes JSON to stdout
- `resolve_project(pool)` -- Resolve active project ID and path from database
- `get_session_modified_files_sync(conn, session_id)` -- Get files modified during a session
- `HookTimer` -- RAII guard that logs hook execution time (warns if >100ms)

## Sub-modules

| Module | Hook Event | Purpose |
|--------|-----------|---------|
| `session` | SessionStart | Initialize Mira session, capture session ID, working directory, and team membership |
| `user_prompt` | UserPromptSubmit | Inject pending tasks, reactive code intelligence, and team context |
| `pre_tool` | PreToolUse | File reread advisory, symbol hints for large files, change pattern warnings |
| `post_tool` | PostToolUse | Track file modifications, queue re-indexing, detect team conflicts |
| `precompact` | PreCompact | Extract decisions, TODOs, and errors from transcript before summarization |
| `stop` | Stop / SessionEnd | Session snapshot, task export, goal progress check |
| `subagent` | SubagentStart/Stop | Inject context for subagents, capture discoveries from subagent work |
| `post_tool_failure` | PostToolFailure | Track tool failures for recurring error detection |
| `task_completed` | TaskCompleted | Log task completions, auto-complete matching goal milestones |
| `teammate_idle` | TeammateIdle | Log teammate idle events for team activity tracking |

## Architecture Notes

Hooks share utility functions from `mod.rs` (database path resolution, project lookup, performance monitoring). The `session` module also provides `read_claude_session_id()` and `read_claude_cwd()` which read from hook-written files in `~/.mira/` to share state between the hook process and the MCP server process.
