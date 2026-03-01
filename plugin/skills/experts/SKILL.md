<!-- plugin/skills/experts/SKILL.md -->
---
name: experts
description: This skill should be used when the user asks to "consult experts", "get expert opinion", "expert review", "code review", "architecture review", "security review", "review this code", "what would an architect say", or wants a multi-perspective analysis from AI specialists.
argument-hint: "[context or focus area]"
---

# Expert Consultation

> **Requires:** Claude Code Agent Teams feature.

Get expert opinions on code, architecture, security, or plans using a team of 4 AI specialists.
Team definition: `.claude/agents/expert-review-team.md`

**Arguments:** $ARGUMENTS

## Instructions

1. **Parse arguments** (optional):
   - `--members nadia,jiro` -> Only spawn these specific experts (by first name)
   - No arguments -> spawn all 4 experts
   - Any other text -> use as the context/question for the experts

2. **Determine context**: The user's question, the code they want reviewed, or the plan they want analyzed. If no context is obvious, ask the user what they'd like experts to review.

3. **Create the team**:
   ```
   TeamCreate(team_name="expert-review-{timestamp}")
   ```

4. **Create and assign tasks**: For each expert being spawned:
   ```
   TaskCreate(subject=task.subject, description=task.description)
   TaskUpdate(taskId=id, owner=member_name, status="in_progress")
   ```

5. **Spawn experts**: For each member (or filtered subset), use the `Task` tool:
   ```
   Task(
     subagent_type="general-purpose",
     name=member_name,
     model="sonnet",
     team_name="expert-review-{timestamp}",
     prompt=member_prompt + "\n\n## Context\n\n" + user_context,
     run_in_background=true
   )
   ```
   Spawn all experts in parallel (multiple Task calls in one message).

   IMPORTANT: Do NOT use mode="bypassPermissions" -- these are read-only discovery agents.
   IMPORTANT: Always pass model="sonnet" to the Task tool. This ensures read-only agents use a cost-efficient model.

   **Member prompts**: Each agent's prompt should include their personality, weakness, focus areas, and allowed tools from the agent file. Instruct them to analyze the context and report findings via SendMessage to the team lead.

6. **Wait for findings**: Teammates will send their findings via SendMessage when complete. Wait for all to finish.

7. **Synthesize findings**: Combine all expert findings into a unified report:
   - **Consensus**: Points multiple experts agree on
   - **Key findings per expert**: Top findings from each specialist
   - **Tensions**: Where experts disagree -- present both sides with evidence
   - **Action items**: Concrete next steps

   IMPORTANT: Preserve genuine disagreements. Do NOT force consensus. Present conditional recommendations: "If your priority is X, then..." / "If your priority is Y, then..."

8. **Cleanup**: Send `shutdown_request` to each teammate, then call `TeamDelete`.

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
