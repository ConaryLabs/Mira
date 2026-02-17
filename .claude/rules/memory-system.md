# Memory System

**Core actions:** `remember`, `recall`, `list`, `forget`, `archive`, `entities`

Use `memory(action="remember", ...)` to store decisions and context. Use `memory(action="recall", ...)` to retrieve them. Use `memory(action="list")` to browse with pagination. Use `memory(action="forget", id=N)` to delete by ID. Use `memory(action="archive", id=N)` to exclude from auto-export while keeping history. Use `memory(action="entities")` to explore the auto-extracted entity graph (technologies, concepts, people linked to memories).

**CLI-only actions:** `export` (structured JSON dump), `purge` (delete all project memories, requires `confirm=true`), `export_claude_local` (write CLAUDE.local.md).

## Evidence Threshold

**Don't store one-off observations.** Only use `remember` for:
- Patterns observed **multiple times** across sessions
- Decisions **explicitly requested** by the user to remember
- Mistakes that caused **real problems** (not hypothetical issues)

When uncertain, don't store it. Memories accumulate and dilute recall quality.

## When to Use Memory

1. **After architectural decisions** - Store the decision and reasoning
2. **User preferences discovered** - Store for future sessions
3. **Mistakes made and corrected** - Remember to avoid repeating
4. **Before making changes** - Recall past decisions in that area
5. **Workflows that worked** - Store successful patterns

Example: User chooses builder pattern for Config -> `memory(action="remember", content="Config struct uses builder pattern. Chosen for clarity and optional field handling.", fact_type="decision", category="patterns")`.

Example: Adding an API endpoint -> first `memory(action="recall", query="API design patterns endpoints conventions")` to follow established patterns.
