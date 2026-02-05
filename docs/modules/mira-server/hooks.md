# hooks

Claude Code hook handlers for lifecycle integration points. Hooks are invoked by Claude Code at specific events.

## I/O

- `read_hook_input()` - Reads JSON input from stdin
- `write_hook_output()` - Writes JSON output to stdout

## Sub-modules

| Module | Hook Event | Purpose |
|--------|-----------|---------|
| `session` | Session start/end | Initialize/finalize Mira session, capture task list ID |
| `pre_tool` | Before tool execution | Inject context before Grep/Glob/Read searches |
| `post_tool` | After tool execution | Track file changes, queue re-indexing |
| `user_prompt` | User prompt submission | Inject context into prompts |
| `precompact` | Before context compaction | Preserve important context |
| `permission` | Permission requests | Handle permission checks |
| `stop` | Session stop | Save state, auto-export CLAUDE.local.md |
| `subagent` | Subagent start/stop | Inject context for subagents, capture discoveries |
