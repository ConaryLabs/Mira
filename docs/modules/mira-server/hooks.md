# hooks

Claude Code hook handlers for lifecycle integration points. Hooks are invoked by Claude Code at specific events.

## I/O

- `read_hook_input()` - Reads JSON input from stdin
- `write_hook_output()` - Writes JSON output to stdout

## Sub-modules

| Module | Hook Event | Purpose |
|--------|-----------|---------|
| `session` | Session start/end | Initialize/finalize Mira session |
| `post_tool` | After tool execution | Process tool results |
| `user_prompt` | User prompt submission | Inject context into prompts |
| `precompact` | Before context compaction | Preserve important context |
| `permission` | Permission requests | Handle permission checks |
| `stop` | Session stop | Save state, auto-export CLAUDE.local.md, check goal progress |
