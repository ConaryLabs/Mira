---
name: experts
description: This skill should be used when the user asks "get expert opinion", "review this", "what would an architect say", "security review", or wants a second opinion from AI experts.
---

# Expert Consultation

Get second opinions from specialized AI experts.

**Question:** $ARGUMENTS

## Instructions

1. Parse the input to extract:
   - **Question/Context**: What to analyze (required)
   - **Roles**: Optional, extract from `--roles X,Y` or infer from question

2. If no roles specified, infer from question:
   - Security-related → `security`
   - Architecture/design → `architect`
   - Code quality → `code_reviewer`
   - Requirements/scope → `scope_analyst`
   - Plan validation → `plan_reviewer`

3. Gather context automatically:
   - Current file being discussed (if any)
   - Recent code changes (if relevant)
   - Related memories from Mira

4. Use the `mcp__mira__expert` tool:
   ```
   expert(action="consult", roles=["architect", "security"], context="...", question="...")
   ```

5. Present expert opinions clearly, noting any disagreements between experts

## Available Roles

| Role | Specialization |
|------|----------------|
| `architect` | System design, patterns, tradeoffs |
| `plan_reviewer` | Validate implementation plans |
| `code_reviewer` | Find bugs, quality issues |
| `security` | Vulnerabilities, hardening |
| `scope_analyst` | Missing requirements, edge cases |

## Examples

```
/mira:experts Should we use a singleton or dependency injection here?
→ expert(action="consult", roles=["architect"], question="Should we use a singleton or dependency injection here?", context="[current file context]")

/mira:experts --roles security,code_reviewer Review this authentication code
→ expert(action="consult", roles=["security", "code_reviewer"], question="Review this authentication code", context="[auth code]")

/mira:experts Is this API design scalable?
→ expert(action="consult", roles=["architect", "scope_analyst"], question="Is this API design scalable?", context="[API design]")
```
