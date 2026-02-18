---
name: experts
description: This skill should be used when the user asks "get expert opinion", "review this", "what would an architect say", "security review", or wants a second opinion from AI experts.
argument-hint: "[context or focus area]"
disable-model-invocation: true
---

# Expert Consultation

> **Requires:** Claude Code Agent Teams feature.

Get expert opinions on code, architecture, security, or plans using a team of AI specialists.

**Arguments:** $ARGUMENTS

## Instructions

1. **Get the recipe**: Call `mcp__mira__recipe` (or `mcp__plugin_mira_mira__recipe`) tool:
   ```
   recipe(action="get", name="expert-review")
   ```

2. **Parse arguments** (optional):
   - `--roles architect,security` → Only spawn these specific experts
   - No arguments → spawn all 7 experts
   - Any other text → use as the context/question for the experts

3. **Determine context**: The user's question, the code they want reviewed, or the plan they want analyzed. If no context is obvious, ask the user what they'd like experts to review.

4. **Create the team**:
   ```
   TeamCreate(team_name="expert-review-{timestamp}")
   ```

5. **Spawn experts**: For each member in the recipe (or filtered subset), use the `Task` tool:
   ```
   Task(
     subagent_type=member.agent_type,
     name=member.name,
     team_name="expert-review-{timestamp}",
     prompt=member.prompt + "\n\n## Context\n\n" + user_context
   )
   ```
   Spawn all experts in parallel (multiple Task calls in one message).

6. **Create and assign tasks**: For each recipe task:
   ```
   TaskCreate(subject=task.subject, description=task.description)
   TaskUpdate(taskId=id, owner=task.assignee, status="in_progress")
   ```

7. **Wait for findings**: Teammates will send their findings via SendMessage when complete. Wait for all to finish.

8. **Synthesize findings**: Combine all expert findings into a unified report:
   - **Consensus**: Points multiple experts agree on
   - **Key findings per expert**: Top findings from each specialist
   - **Tensions**: Where experts disagree — present both sides with evidence
   - **Action items**: Concrete next steps

   IMPORTANT: Preserve genuine disagreements. Do NOT force consensus. Present conditional recommendations: "If your priority is X, then..." / "If your priority is Y, then..."

9. **Cleanup**: Send `shutdown_request` to each teammate, then call `TeamDelete`.

## Examples

```
/mira:experts
→ Prompts for what to review, then spawns all 7 experts

/mira:experts --roles architect,security
→ Only spawns architect and security experts

/mira:experts Review the authentication flow in src/auth/
→ All 7 experts review the auth code

/mira:experts Is this migration plan safe?
→ All 7 experts analyze the plan in context
```

## Expert Roles

| Role | Focus |
|------|-------|
| architect | System design, patterns, tradeoffs |
| code-reviewer | Bugs, logic errors, code quality |
| security | Vulnerabilities, attack vectors |
| scope-analyst | Missing requirements, edge cases |
| ux-strategist | UX, developer experience, API ergonomics |
| plan-reviewer | Plan completeness, risks, gaps |
| growth-strategist | Growth opportunities, adoption, market positioning |
