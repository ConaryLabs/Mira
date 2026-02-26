<!-- .claude/rules/sub-agents.md -->

# Sub-Agent Context Injection

Sub-agents (Task tool with Explore, Plan, etc.) CAN access Mira MCP tools directly
since Claude Code v2.1.30. However, pre-injecting context is still recommended for
efficiency — it avoids extra tool-call round trips inside the sub-agent.

## When to Inject Context

For significant sub-agent work, recall relevant context first:

1. Use `code(action="search", query="...")` to get relevant context
2. Include key findings in the Task prompt
3. The sub-agent can then use Mira tools directly for additional lookups

## When to Skip Injection

For quick exploratory tasks where the sub-agent just needs to search or read code,
launching directly is fine — the sub-agent can call Mira's `code` tool itself.

## Example: Context-Heavy Task

- User asks to plan a caching layer
- First: `code(action="search", query="caching")`, `code(action="search", query="database layer design")`
- Then: launch Plan agent with prompt including constraints (e.g., "Uses SQLite, avoid heavy dependencies")

## Example: Quick Exploration

- User asks "where is authentication handled?"
- Launch Explore agent directly — it can call `code(action="search", query="authentication")` itself
