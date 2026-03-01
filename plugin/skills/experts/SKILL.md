<!-- plugin/skills/experts/SKILL.md -->
---
name: experts
description: This skill should be used when the user asks to "consult experts", "get expert opinion", "expert review", "code review", "architecture review", "security review", "review this code", "what would an architect say", or wants a multi-perspective analysis from AI specialists. Use this for analysis and opinions only — no code changes are made. Use /mira:full-cycle instead if the user wants issues to be automatically implemented.
argument-hint: "[context or focus area]"
---

# Expert Consultation

> **Requires:** Claude Code Agent Teams feature.

Get expert opinions on code, architecture, security, or plans using a team of 4 AI specialists.

**Arguments:** $ARGUMENTS

## Instructions

1. **Parse arguments** (optional):
   - `--members nadia,jiro` -> Only spawn these specific experts (by first name)
   - No arguments -> spawn all 4 experts
   - Any other text -> use as the context/question for the experts

2. **Determine context**: The user's question, the code they want reviewed, or the plan they want analyzed. If no context is obvious, ask the user what they'd like experts to review.

3. **Launch the team**: Call the Mira `launch` MCP tool to get agent specs:
   ```
   launch(team="expert-review-team", scope=user_context, members="nadia,jiro" or omit for all)
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

6. **Spawn experts**: For each agent in `result.data.agents`, use the `Task` tool:
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
   Spawn all experts in parallel (multiple Task calls in one message).

   IMPORTANT: Do NOT use mode="bypassPermissions" -- these are read-only discovery agents.
   IMPORTANT: Always pass model="sonnet" to the Task tool. This ensures read-only agents use a cost-efficient model.

7. **Wait for findings**: Teammates will send their findings via SendMessage when complete. Wait for all to finish.

8. **Synthesize findings**: Combine all expert findings into a unified report:
   - **Consensus**: Points multiple experts agree on
   - **Key findings per expert**: Top findings from each specialist
   - **Tensions**: Where experts disagree -- present both sides with evidence
   - **Action items**: Concrete next steps

   IMPORTANT: Preserve genuine disagreements. Do NOT force consensus. Present conditional recommendations: "If your priority is X, then..." / "If your priority is Y, then..."

9. **Cleanup**: Send `shutdown_request` to each teammate, then call `TeamDelete`.

## Examples

```
/mira:experts
-> Prompts for what to review, then spawns all 4 experts

/mira:experts --members nadia,sable
-> Only spawns Nadia (architect) and Sable (security)

/mira:experts Review the authentication flow in src/auth/
-> All 4 experts review the auth code

/mira:experts Is this migration plan safe?
-> All 4 experts analyze the plan in context
```

## Expert Roles

| Name | Role | Focus |
|------|------|-------|
| Nadia | Systems Architect | Design patterns, API design, coupling, scalability |
| Jiro | Code Quality Reviewer | Bugs, type safety, error handling, race conditions |
| Sable | Security Analyst | SQL injection, auth bypass, input validation, secrets |
| Lena | Scope and Risk Analyst | Missing requirements, edge cases, incomplete error paths |
