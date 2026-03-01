<!-- plugin/skills/full-cycle/SKILL.md -->
---
name: full-cycle
description: This skill should be used when the user asks for a "full review and fix", "find and fix issues", "review and implement", "end-to-end review", "audit and fix", "comprehensive review", "full-cycle review", or wants experts to find issues AND have them automatically implemented (not just reviewed). Use /mira:experts instead if the user only wants opinions or analysis without code changes.
argument-hint: "[focus area or --discovery-only]"
---

# Full-Cycle Review

> **Requires:** Claude Code Agent Teams feature.

End-to-end expert review with automatic implementation and QA verification.

**Arguments:** $ARGUMENTS

## Instructions

1. **Parse arguments** (optional):
   - `--discovery-only` -> Only run Phase 1 (same as `/mira:experts`)
   - `--skip-qa` -> Skip Phase 3 QA verification
   - `--members nadia,sable` -> Only spawn these specific discovery experts (by first name)
   - Any other text -> use as the context/focus for the review

2. **Determine context**: The user's question, the area to review, or the scope of analysis. If no context is obvious, ask the user what they'd like reviewed.

---

### Phase 1: Discovery

3. **Launch discovery team**: Call the Mira `launch` MCP tool to get agent specs:
   ```
   launch(team="expert-review-team", scope=user_context, members="nadia,sable" or omit for all)
   ```
   The `members` parameter is only needed if the user passed `--members`.

4. **Create the team**:
   ```
   TeamCreate(team_name=result.data.suggested_team_id)
   ```

5. **Create and assign discovery tasks**: For each agent in `result.data.agents`:
   ```
   TaskCreate(subject=agent.task_subject, description=agent.task_description)
   TaskUpdate(taskId=id, owner=agent.name, status="in_progress")
   ```

6. **Spawn discovery experts**: For each agent in `result.data.agents`, use the `Task` tool:
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
   Spawn all discovery experts in parallel (multiple Task calls in one message).

   IMPORTANT: Do NOT use `mode="bypassPermissions"` for discovery agents -- they are read-only explorers.
   IMPORTANT: Always pass model="sonnet" to the Task tool. This ensures read-only agents use a cost-efficient model.

7. **Wait for findings**: All discovery experts will send findings via SendMessage. Wait for all to finish, then shut them down.

---

### Phase 2: Synthesis + Implementation

8. **Synthesize findings** into a unified report:
   - **Consensus**: Points multiple experts agree on
   - **Key findings per expert**: Top findings from each specialist
   - **Tensions**: Where experts disagree -- present both sides with evidence
   - **Prioritized action items**: Concrete fixes grouped by file ownership

   IMPORTANT: Preserve genuine disagreements. Do NOT force consensus.

9. **Present synthesis to user** and WAIT for their approval before proceeding to implementation. Do not auto-proceed.

10. **Launch implementation team**: Call `launch` to get implementation agent specs:
    ```
    launch(team="implement-team", scope=approved_items)
    ```

11. **Spawn Kai (implementation planner)** from the launch results:
    ```
    Task(
      subagent_type="general-purpose",
      name="kai",
      model=kai_agent.model,
      team_name=implement_result.data.suggested_team_id,
      prompt=kai_agent.prompt + "\n\n## Approved Findings\n\n" + approved_items,
      run_in_background=true
    )
    ```
    Kai groups fixes by file ownership, identifies dependencies, and sets max 3-5 fixes per agent.

12. **Spawn implementation agents** based on Kai's work breakdown:
    ```
    Task(
      subagent_type="general-purpose",
      name="fixer-{group-name}",
      team_name=implement_result.data.suggested_team_id,  # result from step 10's launch call
      prompt=implementation_prompt + task_descriptions,
      run_in_background=true,
      mode="bypassPermissions"
    )
    ```
    Follow the implement-team coordination rules: strict file ownership, max 3-5 fixes per agent, schema changes first, verify with `cargo test --no-run` (NEVER --release).
    Spawn all implementation agents in parallel. Monitor build diagnostics and send hints if needed.

13. **Spawn Rio (integration verifier)** from the launch results after implementation agents complete:
    ```
    Task(
      subagent_type="general-purpose",
      name="rio",
      model=rio_agent.model,
      team_name=implement_result.data.suggested_team_id,
      prompt=rio_agent.prompt + "\n\n## Changes Made\n\n" + summary_of_changes,
      run_in_background=true,
      mode="bypassPermissions"
    )
    ```
    Rio runs compilation checks, linters, tests, and fixes cross-agent issues.

14. **Wait for implementation**: All agents report completion via SendMessage. Shut them down.

---

### Phase 3: QA Verification

15. **Launch QA team**: Call `launch` to get QA agent specs:
    ```
    launch(team="qa-hardening-team", scope=summary_of_changes)
    ```

16. **Spawn QA agents**: For each agent in the QA launch results:
    ```
    Task(
      subagent_type="general-purpose",
      name=agent.name,
      model=agent.model,
      team_name=qa_result.data.suggested_team_id,  # result from step 15's launch call
      prompt=agent.prompt + "\n\n## Changes Made\n\n" + summary_of_changes,
      run_in_background=true
    )
    ```
    IMPORTANT: Do NOT use `mode="bypassPermissions"` for QA agents -- they are read-only.
    IMPORTANT: Always pass model="sonnet" to the Task tool. This ensures read-only agents use a cost-efficient model.

17. **Create and assign QA tasks** for each auditor.

18. **Wait for QA results**: If issues found, either fix directly or spawn additional fixers.

---

### Phase 4: Finalize

19. **Verify** final build: `cargo clippy --all-targets --all-features -- -D warnings` + `cargo fmt --all -- --check` + `cargo test` (NEVER --release).
20. **Shut down** all remaining agents.
21. **Report** final summary to user with all changes made.
22. **Cleanup**: `TeamDelete`

### Handling Stalled Agents

If an agent has not responded after an unusually long time, send it a direct message via SendMessage to check status. For discovery agents, shut down if unresponsive and note the gap. For implementation agents, fix directly or reassign. Do not wait indefinitely.

## Examples

```
/mira:full-cycle
-> Prompts for what to review, then runs full discovery -> implementation -> QA cycle

/mira:full-cycle Review the database layer for issues
-> 4 experts review the DB layer, findings are implemented, QA verifies

/mira:full-cycle --discovery-only
-> Only runs Phase 1 (equivalent to /mira:experts)

/mira:full-cycle --skip-qa
-> Runs discovery + implementation but skips QA phase

/mira:full-cycle --members nadia,jiro
-> Only Nadia and Jiro run discovery, then full implementation + QA cycle
```

## Phases and Agents

| Phase | Agents | Purpose |
|-------|--------|---------|
| Discovery | Nadia, Jiro, Sable, Lena (expert-review-team) | Find issues, propose improvements |
| Implementation | Kai plans, dynamic agents execute, Rio verifies (implement-team) | Implement fixes in parallel |
| QA | Hana, Orin, Kali, Zara (qa-hardening-team) | Verify changes, catch regressions |
