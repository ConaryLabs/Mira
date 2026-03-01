<!-- plugin/skills/qa-hardening/SKILL.md -->
---
name: qa-hardening
description: This skill should be used when the user asks for "QA review", "production readiness", "hardening pass", "test coverage review", "error handling audit", "pre-release check", or wants to check code quality before release.
argument-hint: "[area to review]"
disable-model-invocation: true
---

# QA Hardening

> **Requires:** Claude Code Agent Teams feature.

Production readiness review with 4 specialists: test health auditor, error handling auditor, security auditor, and edge case hunter.

**Arguments:** $ARGUMENTS

## Instructions

1. **Parse arguments** (optional):
   - `--members hana,kali` -> Only spawn these specific agents (by first name)
   - No arguments -> spawn all 4 agents
   - Any other text -> use as the context/focus for the review

2. **Determine context**: The area to review, specific concerns, or scope of analysis. If no context is obvious, ask the user what they'd like hardened.

3. **Launch the team**: Call the Mira `launch` MCP tool to get agent specs:
   ```
   launch(team="qa-hardening-team", scope=user_context, members="hana,kali" or omit for all)
   ```
   The `members` parameter is only needed if the user passed `--members`. The `scope` parameter should describe the review focus.

4. **Create the team**:
   ```
   TeamCreate(team_name=result.data.suggested_team_id)
   ```

5. **Create and assign tasks**: For each agent in `result.data.agents`:
   ```
   TaskCreate(subject=agent.task_subject, description=agent.task_description)
   TaskUpdate(taskId=id, owner=agent.name, status="in_progress")
   ```

6. **Spawn agents**: For each agent in `result.data.agents`, use the `Task` tool:
   ```
   Task(
     subagent_type="general-purpose",
     name=agent.name,
     model=agent.model,
     team_name=result.data.suggested_team_id,
     prompt=agent.prompt + "\n\n## Context\n\n" + user_context,
     run_in_background=true
   )
   ```
   Spawn all agents in parallel (multiple Task calls in one message).

   IMPORTANT: Do NOT use `mode="bypassPermissions"` -- these are read-only discovery agents.
   IMPORTANT: Always pass model="sonnet" to the Task tool. This ensures read-only agents use a cost-efficient model.

7. **Wait for findings**: All agents will send their findings via SendMessage when complete.

8. **Synthesize findings**: Combine all findings into a prioritized hardening backlog:
   - **Critical** -- Must fix before release (panics in production paths, security holes, data loss)
   - **High** -- Should fix before release (poor error messages, resource leaks, missing validation)
   - **Medium** -- Fix soon after release (coverage gaps, edge cases, docs drift)
   - **Low** -- Polish (naming consistency, minor UX, nice-to-have tests)
   - **Deferred** -- Needs design discussion (architectural changes, large refactors)

   Cross-reference findings -- when multiple agents flag the same area, elevate priority.

9. **Cleanup**: Send `shutdown_request` to each teammate, then call `TeamDelete`.

## Want findings implemented?

After presenting the hardening backlog, ask the user if they want fixes implemented. If yes, use `launch(team="implement-team", scope=approved_items)` to get implementation agent specs, then spawn Kai for planning, parallel fixers for execution, and Rio for verification.

## Examples

```
/mira:qa-hardening
-> Prompts for what to review, then spawns all 4 agents

/mira:qa-hardening Review the recipe system
-> All 4 agents review the recipe code for production readiness

/mira:qa-hardening --members kali,orin
-> Only spawns Kali (security) and Orin (error-handling)
```

## Agent Roles

| Name | Role | Focus |
|------|------|-------|
| Hana | Test Health Auditor | Test suite health, coverage gaps, build quality |
| Orin | Error Handling Auditor | Panic paths, error messages, recovery |
| Kali | Security Auditor | Input validation, data exposure, auth bypass |
| Zara | Edge Case Hunter | Boundary conditions, concurrency, resource exhaustion |
