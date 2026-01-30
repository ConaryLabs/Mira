# Expert Consultation

Use the unified `consult_experts` tool for second opinions before major decisions:

```
consult_experts(roles=["architect"], context="...", question="...")
consult_experts(roles=["code_reviewer", "security"], context="...")  # Multiple experts in parallel
```

## Available Roles

- `architect` - system design, patterns, tradeoffs
- `plan_reviewer` - validate plans before coding
- `code_reviewer` - find bugs, quality issues
- `security` - vulnerabilities, hardening
- `scope_analyst` - missing requirements, edge cases

## When to Consult

1. **Before major refactoring** - `architect`
2. **After writing implementation plan** - `plan_reviewer`
3. **Before merging significant changes** - `code_reviewer`
4. **When handling user input or auth** - `security`
5. **When requirements seem incomplete** - `scope_analyst`
