# Sub-Agent Context Injection

Sub-agents (Task tool with Explore, Plan, etc.) do NOT automatically have access to Mira memories. You must inject relevant context into the prompt.

## Pattern: Recall Before Task

Before launching a sub-agent for significant work:

1. Use `recall()` to get relevant context for the domain
2. Include key findings in the Task prompt
3. Be explicit about project conventions and constraints

Example flow:
- User asks to explore error handling
- First: `recall("error handling patterns conventions")`
- Then: launch Explore agent with prompt including the recalled context (e.g., "Project uses thiserror, custom MiraError enum in types crate")
- The agent can now search more effectively with that knowledge

Example flow:
- User asks to plan a caching layer
- First: `recall("caching")`, `recall("database layer design")`
- Then: launch Plan agent with prompt including constraints (e.g., "Uses SQLite, avoid heavy dependencies")
