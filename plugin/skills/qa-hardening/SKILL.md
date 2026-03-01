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
Team definition: `.claude/agents/qa-hardening-team.md`

**Arguments:** $ARGUMENTS

## Instructions

1. **Parse arguments** (optional):
   - `--members hana,kali` -> Only spawn these specific agents (by first name)
   - No arguments -> spawn all 4 agents
   - Any other text -> use as the context/focus for the review

2. **Determine context**: The area to review, specific concerns, or scope of analysis. If no context is obvious, ask the user what they'd like hardened.

3. **Create the team**:
   ```
   TeamCreate(team_name="qa-hardening-{timestamp}")
   ```

4. **Create tasks FIRST**: For each agent being spawned, create with `TaskCreate` before spawning agents.

5. **Spawn agents**: For each member (or filtered subset), use the `Task` tool:
   ```
   Task(
     subagent_type="general-purpose",
     name=member_name,
     model="sonnet",
     team_name="qa-hardening-{timestamp}",
     prompt=member_prompt + "\n\n## Context\n\n" + user_context,
     run_in_background=true
   )
   ```
   Spawn all agents in parallel (multiple Task calls in one message).
   IMPORTANT: Do NOT use `mode="bypassPermissions"` -- these are read-only discovery agents.
   IMPORTANT: Always pass model="sonnet" to the Task tool. This ensures read-only agents use a cost-efficient model.

   **Member prompts**: Each agent's prompt should include their personality, weakness, focus areas, and allowed tools from the agent file. Instruct them to analyze the context and report findings via SendMessage to the team lead.

6. **Assign tasks**: Use `TaskUpdate` to assign each task to its corresponding agent.

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

After presenting the hardening backlog, ask the user if they want fixes implemented. If yes, spawn implementation agents following the implement-team coordination pattern (`.claude/agents/implement-team.md`): Kai plans the work breakdown, parallel agents execute with file ownership, Rio verifies.

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
