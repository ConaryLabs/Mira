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

**Arguments:** $ARGUMENTS

## Instructions

1. **Parse arguments** (optional):
   - Any text -> use as the refactoring goal/context for the architect

2. **Determine context**: What code to refactor, the goal of the restructuring, and any constraints. If no context is obvious, ask the user what they'd like refactored.

---

### Phase 1: Analysis

3. **Launch for analysis**: Call the Mira `launch` MCP tool to get the architect agent spec:
   ```
   launch(team="refactor-team", scope=user_context, members="atlas")
   ```

4. **Create the team**:
   ```
   TeamCreate(team_name=result.data.suggested_team_id)
   ```

5. **Create and assign** the analysis task:
   ```
   TaskCreate(subject=atlas_agent.task_subject, description=atlas_agent.task_description)
   TaskUpdate(taskId=id, owner="atlas", status="in_progress")
   ```

6. **Spawn Atlas (architect)** using `Task` tool:
   ```
   Task(
     subagent_type="general-purpose",
     name="atlas",
     model=atlas_agent.model,
     team_name=result.data.suggested_team_id,
     prompt=atlas_agent.prompt + "\n\n## Refactoring Goal\n\n" + user_context,
     run_in_background=true
   )
   ```
   IMPORTANT: Do NOT use `mode="bypassPermissions"` -- Atlas is read-only.
   IMPORTANT: Always pass model="sonnet" to the Task tool. This ensures read-only agents use a cost-efficient model.

7. **Wait** for Atlas to report via SendMessage.

---

### Phase 2: Validation

8. **Launch for validation**: Call `launch` to get the reviewer agent spec:
   ```
   launch(team="refactor-team", scope=user_context, members="iris")
   ```

9. **Spawn Iris (safety reviewer)** using `Task` tool:
   ```
   Task(
     subagent_type="general-purpose",
     name="iris",
     model=iris_agent.model,
     team_name=result.data.suggested_team_id,
     prompt=iris_agent.prompt + "\n\n## Refactoring Plan\n\n" + atlas_plan,
     run_in_background=true
   )
   ```
   IMPORTANT: Do NOT use `mode="bypassPermissions"` -- Iris is read-only.
   IMPORTANT: Always pass model="sonnet" to the Task tool. This ensures read-only agents use a cost-efficient model.

10. **Send Atlas's plan** to Iris via `SendMessage` so she has the specific plan to validate.

11. **Create and assign** the validation task.

12. **Wait** for Iris to report via SendMessage. Then shut down both analyst agents.

---

### Phase 3: Synthesis

13. **Combine** Atlas's plan with Iris's feedback:
    - If Iris found missing callers or risks, incorporate them
    - If rated "needs revision" or "too risky", revise accordingly

14. **Present** the validated refactoring plan to the user and **WAIT for approval**.

---

### Phase 4: Implementation

15. **Execute the refactoring steps** yourself. For each step:
    - Make the changes
    - Run `cargo test --no-run` to verify compilation (NEVER use --release)
    - If it doesn't compile, fix before moving to the next step

16. For **large refactors** (5+ files), launch implementation agents:
    ```
    launch(team="refactor-team", scope=implementation_plan, members="ash")
    ```
    Spawn implementation agents with `mode="bypassPermissions"`:
    - Group steps by file ownership to avoid conflicts
    - Max 3 steps per agent
    - Each agent verifies with `cargo test --no-run`

---

### Phase 5: Verification

17. **Launch for verification**: Call `launch` to get the verifier agent spec:
    ```
    launch(team="refactor-team", scope=summary_of_changes, members="ash")
    ```

18. **Spawn Ash (build verifier)** using `Task` tool:
    ```
    Task(
      subagent_type="general-purpose",
      name="ash",
      model=ash_agent.model,
      team_name=result.data.suggested_team_id,
      prompt=ash_agent.prompt + "\n\n## Changes Made\n\n" + summary_of_changes,
      run_in_background=true,
      mode="bypassPermissions"
    )
    ```

19. **Wait** for Ash to report. If tests fail: fix regressions or update test imports/paths.

---

### Phase 6: Finalize

20. **Report** summary of structural changes to the user.
21. **Cleanup**: Send `shutdown_request` to remaining agents, then `TeamDelete`.

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
