# Expert Consultation

Use the unified `expert` tool for second opinions before major decisions:

```
expert(action="consult", roles=["architect"], context="...", question="...")
expert(action="consult", roles=["code_reviewer", "security"], context="...")  # Multiple experts in parallel
```

Configure experts with:
```
expert(action="configure", config_action="list")
expert(action="configure", config_action="set", role="architect", provider="deepseek", model="deepseek-reasoner")
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
