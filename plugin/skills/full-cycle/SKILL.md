---
name: full-cycle
description: This skill should be used when the user asks for a "full review and fix", "find and fix issues", "review and implement", "end-to-end review", or wants experts to find issues AND have them implemented automatically.
---

# Full-Cycle Review

End-to-end expert review with automatic implementation and QA verification.

**Arguments:** $ARGUMENTS

## Instructions

1. **Get the recipe**: Call `mcp__mira__recipe` (or `mcp__plugin_mira_mira__recipe`) tool:
   ```
   recipe(action="get", name="full-cycle")
   ```

2. **Parse arguments** (optional):
   - `--discovery-only` -> Only run Phase 1 (same as `/mira:experts`)
   - `--skip-qa` -> Skip Phase 3 QA verification
   - `--roles architect,security` -> Only spawn these specific discovery experts
   - Any other text -> use as the context/focus for the review

3. **Determine context**: The user's question, the area to review, or the scope of analysis. If no context is obvious, ask the user what they'd like reviewed.

---

### Phase 1: Discovery

4. **Create the team**:
   ```
   TeamCreate(team_name="full-cycle-{timestamp}")
   ```

5. **Spawn discovery experts** (members with names: architect, code-reviewer, security, scope-analyst, ux-strategist, plan-reviewer). Use `Task` tool for each:
   ```
   Task(
     subagent_type=member.agent_type,
     name=member.name,
     team_name="full-cycle-{timestamp}",
     prompt=member.prompt + "\n\n## Context\n\n" + user_context,
     run_in_background=true
   )
   ```
   Spawn all discovery experts in parallel (multiple Task calls in one message).
   IMPORTANT: Do NOT use `mode="bypassPermissions"` for discovery agents — they are read-only explorers.

6. **Create and assign discovery tasks**: For each discovery task in the recipe:
   ```
   TaskCreate(subject=task.subject, description=task.description)
   TaskUpdate(taskId=id, owner=task.assignee, status="in_progress")
   ```

7. **Wait for findings**: All 6 discovery experts will send findings via SendMessage. Wait for all to finish, then shut them down.

---

### Phase 2: Synthesis + Implementation

8. **Synthesize findings** into a unified report:
   - **Consensus**: Points multiple experts agree on
   - **Key findings per expert**: Top findings from each specialist
   - **Tensions**: Where experts disagree -- present both sides with evidence
   - **Prioritized action items**: Concrete fixes grouped by file ownership

   IMPORTANT: Preserve genuine disagreements. Do NOT force consensus.

9. **Present synthesis to user** and WAIT for their approval before proceeding to implementation. Do not auto-proceed.

10. **Create implementation tasks** from the action items. Group tasks by file ownership to prevent merge conflicts between agents.

11. **Spawn implementation agents** dynamically (as many as needed based on groupings):
    ```
    Task(
      subagent_type="general-purpose",
      name="fixer-{group-name}",
      team_name="full-cycle-{timestamp}",
      prompt="You are a teammate on an implementation team...\n\nIMPORTANT:\n- NEVER use cargo build --release or cargo test --release. Always use debug mode.\n- Verify your changes with `cargo clippy --all-targets --all-features -- -D warnings` AND `cargo fmt`, not just `cargo build`.\n- When fixing pattern issues (e.g., inconsistent error messages), search the ENTIRE codebase for all instances, not just the files listed.\n\n" + task_descriptions,
      run_in_background=true,
      mode="bypassPermissions"
    )
    ```
    Spawn all implementation agents in parallel. Monitor build diagnostics and send hints if needed.

12. **Wait for implementation**: All agents report completion via SendMessage. Shut them down.

---

### Phase 3: QA Verification

13. **Spawn QA agents** (test-runner, ux-reviewer) with context about what was changed:
    ```
    Task(
      subagent_type="general-purpose",
      name=member.name,
      team_name="full-cycle-{timestamp}",
      prompt=member.prompt + "\n\n## Changes Made\n\n" + summary_of_changes,
      run_in_background=true,
      mode="bypassPermissions"
    )
    ```

14. **Create and assign QA tasks** from the recipe.

15. **Wait for QA results**: If issues found, either fix directly or spawn additional fixers.

---

### Phase 4: Finalize

16. **Verify** final build (`cargo build`) and tests (`cargo test`).
17. **Shut down** all remaining agents.
18. **Report** final summary to user with all changes made.
19. **Cleanup**: `TeamDelete`

## Examples

```
/mira:full-cycle
-> Prompts for what to review, then runs full discovery -> implementation -> QA cycle

/mira:full-cycle Review the database layer for issues
-> All 6 experts review the DB layer, findings are implemented, QA verifies

/mira:full-cycle --discovery-only
-> Only runs Phase 1 (equivalent to /mira:experts with ux-strategist added)

/mira:full-cycle --skip-qa
-> Runs discovery + implementation but skips QA phase
```

## Phases & Agents

| Phase | Agents | Purpose |
|-------|--------|---------|
| Discovery | architect, code-reviewer, security, scope-analyst, ux-strategist, plan-reviewer | Find issues, propose improvements |
| Implementation | dynamic (fixer-security, fixer-bugs, etc.) | Implement fixes in parallel |
| QA | test-runner, ux-reviewer | Verify changes, catch regressions |
