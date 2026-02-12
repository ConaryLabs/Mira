---
name: qa-hardening
description: This skill should be used when the user asks for "QA review", "production readiness", "hardening pass", "test coverage review", "error handling audit", or wants to check code quality before release.
argument-hint: "[area to review]"
disable-model-invocation: true
---

# QA Hardening

> **Requires:** Claude Code Agent Teams feature.

Production readiness review with 5 specialists: test runner, error auditor, security, edge case hunter, and UX reviewer.

**Arguments:** $ARGUMENTS

## Instructions

1. **Get the recipe**: Call `mcp__mira__recipe` (or `mcp__plugin_mira_mira__recipe`) tool:
   ```
   recipe(action="get", name="qa-hardening")
   ```

2. **Parse arguments** (optional):
   - `--roles test-runner,security` -> Only spawn these specific agents
   - No arguments -> spawn all 5 agents
   - Any other text -> use as the context/focus for the review

3. **Determine context**: The area to review, specific concerns, or scope of analysis. If no context is obvious, ask the user what they'd like hardened.

4. **Create the team**:
   ```
   TeamCreate(team_name="qa-hardening-{timestamp}")
   ```

5. **Create tasks FIRST**: For each recipe task, create with `TaskCreate` before spawning agents.

6. **Spawn agents**: For each member in the recipe (or filtered subset), use the `Task` tool:
   ```
   Task(
     subagent_type=member.agent_type,
     name=member.name,
     team_name="qa-hardening-{timestamp}",
     prompt=member.prompt + "\n\n## Context\n\n" + user_context,
     run_in_background=true
   )
   ```
   Spawn all agents in parallel (multiple Task calls in one message).
   IMPORTANT: Do NOT use `mode="bypassPermissions"` — these are read-only discovery agents.

7. **Assign tasks**: Use `TaskUpdate` to assign each task to its corresponding agent.

8. **Wait for findings**: All agents will send their findings via SendMessage when complete.

9. **Synthesize findings**: Combine all findings into a prioritized hardening backlog:
   - **Critical** — Must fix before release (panics in production paths, security holes, data loss)
   - **High** — Should fix before release (poor error messages, resource leaks, missing validation)
   - **Medium** — Fix soon after release (coverage gaps, edge cases, docs drift)
   - **Low** — Polish (naming consistency, minor UX, nice-to-have tests)
   - **Deferred** — Needs design discussion (architectural changes, large refactors)

   Cross-reference findings — when multiple agents flag the same area, elevate priority.

10. **Cleanup**: Send `shutdown_request` to each teammate, then call `TeamDelete`.

## Want findings implemented?

After presenting the hardening backlog, ask the user if they want fixes implemented. If yes, follow Phase 3 from the recipe's coordination instructions to spawn implementation agents.

## Examples

```
/mira:qa-hardening
-> Prompts for what to review, then spawns all 5 agents

/mira:qa-hardening Review the recipe system
-> All 5 agents review the recipe code for production readiness

/mira:qa-hardening --roles security,error-auditor
-> Only spawns security and error-auditor agents
```

## Agent Roles

| Role | Focus |
|------|-------|
| test-runner | Test suite health, coverage gaps, build quality |
| error-auditor | Error handling, panic safety, error message quality |
| security | Input validation, data exposure, secure defaults |
| edge-case-hunter | Boundary conditions, resource management, concurrency |
| ux-reviewer | API ergonomics, error experience, documentation freshness |
