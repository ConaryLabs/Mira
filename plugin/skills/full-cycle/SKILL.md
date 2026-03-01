<!-- plugin/skills/full-cycle/SKILL.md -->
---
name: full-cycle
description: This skill should be used when the user asks for a "full review and fix", "find and fix issues", "review and implement", "end-to-end review", "audit and fix", "comprehensive review", "full-cycle review", or wants experts to find issues AND have them implemented automatically.
argument-hint: "[focus area or --discovery-only]"
---

# Full-Cycle Review

> **Requires:** Claude Code Agent Teams feature.

End-to-end expert review with automatic implementation and QA verification.
Team definitions: `.claude/agents/expert-review-team.md`, `.claude/agents/implement-team.md`, `.claude/agents/qa-hardening-team.md`

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

Uses the **expert-review-team** (`.claude/agents/expert-review-team.md`).

3. **Create the team**:
   ```
   TeamCreate(team_name="full-cycle-{timestamp}")
   ```

4. **Spawn discovery experts** (Nadia, Jiro, Sable, Lena). Use `Task` tool for each:
   ```
   Task(
     subagent_type="general-purpose",
     name=member_name,
     model="sonnet",
     team_name="full-cycle-{timestamp}",
     prompt=member_prompt + "\n\n## Context\n\n" + user_context,
     run_in_background=true
   )
   ```
   Spawn all 4 discovery experts in parallel (multiple Task calls in one message).
   IMPORTANT: Do NOT use `mode="bypassPermissions"` for discovery agents -- they are read-only explorers.
   IMPORTANT: Always pass model="sonnet" to the Task tool. This ensures read-only agents use a cost-efficient model.

   **Member prompts**: Each agent's prompt should include their personality, weakness, focus areas, and allowed tools from the expert-review-team agent file. Instruct them to analyze the context and report findings via SendMessage to the team lead.

5. **Create and assign discovery tasks**: For each expert:
   ```
   TaskCreate(subject=task.subject, description=task.description)
   TaskUpdate(taskId=id, owner=member_name, status="in_progress")
   ```

6. **Wait for findings**: All 4 discovery experts will send findings via SendMessage. Wait for all to finish, then shut them down.

---

### Phase 2: Synthesis + Implementation

Uses the **implement-team** coordination pattern (`.claude/agents/implement-team.md`).

7. **Synthesize findings** into a unified report:
   - **Consensus**: Points multiple experts agree on
   - **Key findings per expert**: Top findings from each specialist
   - **Tensions**: Where experts disagree -- present both sides with evidence
   - **Prioritized action items**: Concrete fixes grouped by file ownership

   IMPORTANT: Preserve genuine disagreements. Do NOT force consensus.

8. **Present synthesis to user** and WAIT for their approval before proceeding to implementation. Do not auto-proceed.

9. **Spawn Kai (implementation planner)** to analyze the approved findings and produce a work breakdown:
   ```
   Task(
     subagent_type="general-purpose",
     name="kai",
     model="sonnet",
     team_name="full-cycle-{timestamp}",
     prompt=kai_prompt + "\n\n## Approved Findings\n\n" + approved_items,
     run_in_background=true
   )
   ```
   Kai groups fixes by file ownership, identifies dependencies, and sets max 3-5 fixes per agent.

10. **Spawn implementation agents** based on Kai's work breakdown:
    ```
    Task(
      subagent_type="general-purpose",
      name="fixer-{group-name}",
      team_name="full-cycle-{timestamp}",
      prompt=implementation_prompt + task_descriptions,
      run_in_background=true,
      mode="bypassPermissions"
    )
    ```
    Follow the implement-team coordination rules: strict file ownership, max 3-5 fixes per agent, schema changes first, verify with `cargo test --no-run` (NEVER --release).
    Spawn all implementation agents in parallel. Monitor build diagnostics and send hints if needed.

11. **Spawn Rio (integration verifier)** after implementation agents complete:
    ```
    Task(
      subagent_type="general-purpose",
      name="rio",
      team_name="full-cycle-{timestamp}",
      prompt=rio_prompt + "\n\n## Changes Made\n\n" + summary_of_changes,
      run_in_background=true,
      mode="bypassPermissions"
    )
    ```
    Rio runs compilation checks, linters, tests, and fixes cross-agent issues.

12. **Wait for implementation**: All agents report completion via SendMessage. Shut them down.

---

### Phase 3: QA Verification

Uses the **qa-hardening-team** (`.claude/agents/qa-hardening-team.md`).

13. **Spawn QA agents** (Hana, Orin, Kali, Zara) with context about what was changed:
    ```
    Task(
      subagent_type="general-purpose",
      name=member_name,
      model="sonnet",
      team_name="full-cycle-{timestamp}",
      prompt=member_prompt + "\n\n## Changes Made\n\n" + summary_of_changes,
      run_in_background=true,
      mode="bypassPermissions"
    )
    ```

    **Member prompts**: Each agent's prompt should include their personality, weakness, focus areas, and allowed tools from the qa-hardening-team agent file.

14. **Create and assign QA tasks** for each auditor.

15. **Wait for QA results**: If issues found, either fix directly or spawn additional fixers.

---

### Phase 4: Finalize

16. **Verify** final build: `cargo clippy --all-targets --all-features -- -D warnings` + `cargo fmt --all -- --check` + `cargo test` (NEVER --release).
17. **Shut down** all remaining agents.
18. **Report** final summary to user with all changes made.
19. **Cleanup**: `TeamDelete`

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
