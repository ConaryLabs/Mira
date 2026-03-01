<!-- plugin/skills/refactor/SKILL.md -->
---
name: refactor
description: This skill should be used when the user asks to "refactor this", "restructure code", "reorganize modules", "rename and move", "safe restructuring", "reorganize code", "move and rename", or wants to reorganize code without changing behavior.
argument-hint: "[refactoring goal]"
disable-model-invocation: true
---

# Safe Refactoring

> **Requires:** Claude Code Agent Teams feature.

Architect-planned, reviewer-validated code restructuring with per-step compilation checks.
Team definition: `.claude/agents/refactor-team.md`

**Arguments:** $ARGUMENTS

## Instructions

1. **Parse arguments** (optional):
   - Any text -> use as the refactoring goal/context for the architect

2. **Determine context**: What code to refactor, the goal of the restructuring, and any constraints. If no context is obvious, ask the user what they'd like refactored.

---

### Phase 1: Analysis

3. **Create the team**:
   ```
   TeamCreate(team_name="refactor-{timestamp}")
   ```

4. **Spawn Atlas (architect)** using `Task` tool:
   ```
   Task(
     subagent_type="general-purpose",
     name="atlas",
     model="sonnet",
     team_name="refactor-{timestamp}",
     prompt=atlas_prompt + "\n\n## Refactoring Goal\n\n" + user_context,
     run_in_background=true
   )
   ```
   IMPORTANT: Do NOT use `mode="bypassPermissions"` -- Atlas is read-only.

   **Atlas prompt**: Include personality, weakness, focus areas, and allowed tools from the agent file. Instruct Atlas to map current structure, design the target, and plan migration as a sequence of small, independently-compilable steps.

5. **Create and assign** the analysis task using `TaskCreate` + `TaskUpdate`.

6. **Wait** for Atlas to report via SendMessage.

---

### Phase 2: Validation

7. **Spawn Iris (safety reviewer)** using `Task` tool (same params: model="sonnet", run_in_background=true, no bypassPermissions).

   **Iris prompt**: Include personality, weakness, focus areas, and allowed tools from the agent file. Instruct Iris to review the refactoring plan for behavior preservation, caller updates, and side-effect ordering.

8. **Send Atlas's plan** to Iris via `SendMessage` so she has the specific plan to validate.

9. **Create and assign** the validation task.

10. **Wait** for Iris to report via SendMessage. Then shut down both analyst agents.

---

### Phase 3: Synthesis

11. **Combine** Atlas's plan with Iris's feedback:
    - If Iris found missing callers or risks, incorporate them
    - If rated "needs revision" or "too risky", revise accordingly

12. **Present** the validated refactoring plan to the user and **WAIT for approval**.

---

### Phase 4: Implementation

13. **Execute the refactoring steps** yourself. For each step:
    - Make the changes
    - Run `cargo test --no-run` to verify compilation (NEVER use --release)
    - If it doesn't compile, fix before moving to the next step

14. For **large refactors** (5+ files), spawn implementation agents with `mode="bypassPermissions"`:
    - Group steps by file ownership to avoid conflicts
    - Max 3 steps per agent
    - Each agent verifies with `cargo test --no-run`

---

### Phase 5: Verification

15. **Spawn Ash (build verifier)** using `Task` tool:
    ```
    Task(
      subagent_type="general-purpose",
      name="ash",
      model="sonnet",
      team_name="refactor-{timestamp}",
      prompt=ash_prompt + "\n\n## Changes Made\n\n" + summary_of_changes,
      run_in_background=true,
      mode="bypassPermissions"
    )
    ```

    **Ash prompt**: Include personality, weakness, focus areas, and allowed tools from the agent file. Instruct Ash to run compilation checks, tests, linters, and report any failures.

16. **Wait** for Ash to report. If tests fail: fix regressions or update test imports/paths.

---

### Phase 6: Finalize

17. **Report** summary of structural changes to the user.
18. **Cleanup**: Send `shutdown_request` to remaining agents, then `TeamDelete`.

## Examples

```
/mira:refactor
-> Prompts for what to refactor, then runs the full analysis -> validation -> implementation cycle

/mira:refactor Split the database module into separate files per concern
-> Atlas analyzes the DB module, plans the split, Iris validates, then implements

/mira:refactor Rename the "watcher" module to "file_monitor" across the codebase
-> Safe rename with caller analysis and per-step compilation checks
```

## Agent Roles

| Phase | Agent | Focus |
|-------|-------|-------|
| Analysis | Atlas (architect) | Map current structure, design target, plan migration steps |
| Validation | Iris (safety reviewer) | Verify completeness, check for hidden behavior changes |
| Verification | Ash (build verifier) | Run tests, compilation, linters between steps |
