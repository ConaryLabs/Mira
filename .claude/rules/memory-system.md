# Memory System: Remember & Recall

Use `remember` to store decisions and context. Use `recall` to retrieve them.

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

Example: User chooses builder pattern for Config -> `remember(content="Config struct uses builder pattern. Chosen for clarity and optional field handling.", category="decision")`.

Example: Adding an API endpoint -> first `recall("API design patterns endpoints conventions")` to follow established patterns.
