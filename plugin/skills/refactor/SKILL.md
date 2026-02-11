---
name: refactor
description: This skill should be used when the user asks to "refactor this", "restructure code", "reorganize modules", "rename and move", "safe restructuring", or wants to reorganize code without changing behavior.
---

# Safe Refactoring

> **Requires:** Claude Code Agent Teams feature.

Architect-planned, reviewer-validated code restructuring with per-step compilation checks.

**Arguments:** $ARGUMENTS

## Instructions

1. **Get the recipe**: Call `mcp__mira__recipe` (or `mcp__plugin_mira_mira__recipe`) tool:
   ```
   recipe(action="get", name="refactor")
   ```

2. **Parse arguments** (optional):
   - Any text -> use as the refactoring goal/context for the architect

3. **Determine context**: What code to refactor, the goal of the restructuring, and any constraints. If no context is obvious, ask the user what they'd like refactored.

---

### Phase 1: Analysis

4. **Create the team**:
   ```
   TeamCreate(team_name="refactor-{timestamp}")
   ```

5. **Spawn architect** using `Task` tool:
   ```
   Task(
     subagent_type="general-purpose",
     name="architect",
     team_name="refactor-{timestamp}",
     prompt=architect_prompt + "\n\n## Refactoring Goal\n\n" + user_context,
     run_in_background=true
   )
   ```
   IMPORTANT: Do NOT use `mode="bypassPermissions"` — the architect is read-only.

6. **Create and assign** the analysis task using `TaskCreate` + `TaskUpdate`.

7. **Wait** for the architect to report via SendMessage.

---

### Phase 2: Validation

8. **Spawn code-reviewer** using `Task` tool (same params, `run_in_background=true`, no `bypassPermissions`).

9. **Send the architect's plan** to code-reviewer via `SendMessage` so they have the specific plan to validate.

10. **Create and assign** the validation task.

11. **Wait** for code-reviewer to report via SendMessage. Then shut down both analyst agents.

---

### Phase 3: Synthesis

12. **Combine** architect's plan with code-reviewer's feedback:
    - If code-reviewer found missing callers or risks, incorporate them
    - If rated "needs revision" or "too risky", revise accordingly

13. **Present** the validated refactoring plan to the user and **WAIT for approval**.

---

### Phase 4: Implementation

14. **Execute the refactoring steps** yourself. For each step:
    - Make the changes
    - Run `cargo test --no-run` to verify compilation (NEVER use --release)
    - If it doesn't compile, fix before moving to the next step

15. For **large refactors** (5+ files), spawn implementation agents with `mode="bypassPermissions"`:
    - Group steps by file ownership to avoid conflicts
    - Max 3 steps per agent
    - Each agent verifies with `cargo test --no-run`

---

### Phase 5: Verification

16. **Run `cargo test`** to verify all tests pass (NEVER use --release).
17. For large refactors, optionally spawn test-runner for parallel clippy + fmt + dead code checks.
18. If tests fail: fix regressions or update test imports/paths.

---

### Phase 6: Finalize

19. **Report** summary of structural changes to the user.
20. **Cleanup**: `TeamDelete`

## Examples

```
/mira:refactor
-> Prompts for what to refactor, then runs the full analysis -> validation -> implementation cycle

/mira:refactor Split the database module into separate files per concern
-> Architect analyzes the DB module, plans the split, code-reviewer validates, then implements

/mira:refactor Rename the "watcher" module to "file_monitor" across the codebase
-> Safe rename with caller analysis and per-step compilation checks
```

## Agent Roles

| Phase | Agent | Focus |
|-------|-------|-------|
| Analysis | architect | Map current structure, design target, plan migration steps |
| Validation | code-reviewer | Verify completeness, check for hidden behavior changes |
| Verification | test-runner | Run tests, clippy, fmt, check for dead code (large refactors only) |
