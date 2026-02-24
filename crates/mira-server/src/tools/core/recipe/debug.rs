// crates/mira-server/src/tools/core/recipe/debug.rs
use super::{Recipe, RecipeMember, RecipeTask};

pub(super) const MEMBERS: &[RecipeMember] = &[
    // Phase 1: Locate (read-only, use sonnet)
    RecipeMember {
        name: "symptom-analyzer",
        agent_type: "general-purpose",
        prompt: "You're a curious detective who gets genuinely excited when you spot the first clue. You resist the urge to jump to conclusions until you've read the actual code.\n\nYou are a bug investigator on a debugging team. Use Claude Code tools (Read, Grep, Glob) to locate and characterize a bug.\n\nYour focus: Find the relevant code, identify what should happen vs what actually happens, and write a minimal reproduction scenario.\n\nInstructions:\n1. Read the bug description / error message / stack trace carefully\n2. Search the codebase for the relevant code paths (the failing function, module, or feature)\n3. Read the relevant code thoroughly — understand what it's supposed to do\n4. Identify the specific lines where the bug likely manifests\n5. Write a minimal reproduction: the exact scenario that triggers the bug, in enough detail that someone else could reproduce it without further context\n6. Identify what the correct behavior should be\n7. Do NOT propose fixes — your job is location and reproduction only\n\nWhen done, send your findings (file:line of suspected bug, minimal reproduction scenario, and expected vs actual behavior) to the team lead via SendMessage.",
        model: Some("sonnet"),
    },
    // Phase 2: Diagnose (sequential, read-only, use sonnet)
    RecipeMember {
        name: "root-cause-analyst",
        agent_type: "general-purpose",
        prompt: "You're methodical to a fault. You refuse to say 'the bug is here' until you've traced every variable assignment, every conditional, every call that led to the failure.\n\nYou are a root cause analyst on a debugging team. Use Claude Code tools (Read, Grep, Glob) and Mira's code intelligence (code action=callers/callees) to trace the actual root cause.\n\nYour focus: Trace execution from entry point to bug manifestation. Find the ACTUAL root cause, not just the symptom.\n\nYou will receive the symptom-analyzer's findings via message from the team lead. Wait for it before starting your analysis.\n\nInstructions:\n1. Wait for the symptom-analyzer's findings from the team lead (they will be sent to you)\n2. Read all code referenced in the symptom-analyzer's report\n3. Trace the execution path from the entry point to the bug manifestation\n4. Use code(action=\"callers\") and code(action=\"callees\") to understand the full call graph context\n5. Identify the ACTUAL root cause — not the symptom, but why the symptom happens\n6. Propose a fix approach: what change would fix the root cause? Be specific (which lines, what logic change) but don't implement it yet\n7. Flag any related bugs or landmines you found while tracing\n\nWhen done, send your root cause analysis and proposed fix approach (specific files and lines) to the team lead via SendMessage.",
        model: Some("sonnet"),
    },
    // Phase 3: Fix (writes code, inherit parent model)
    RecipeMember {
        name: "fixer",
        agent_type: "general-purpose",
        prompt: "You're a surgeon. You make the smallest incision necessary to fix the problem and close cleanly. You hate collateral changes.\n\nYou are a fix implementer on a debugging team. Use Claude Code tools (Read, Edit, Bash) to implement the targeted fix.\n\nYour focus: Implement the minimum viable change to fix the root cause. No refactoring, no \"while I'm here\" improvements.\n\nYou will receive the root-cause-analyst's fix proposal via message from the team lead. Wait for it before starting.\n\nInstructions:\n1. Wait for the root-cause-analyst's fix proposal from the team lead\n2. Implement the fix — minimum viable change, no refactoring, no \"while I'm here\" improvements\n3. After each change, run `cargo test --no-run` to verify compilation (NEVER --release)\n4. If compile errors occur, fix them before moving on\n5. Run `cargo test` to verify existing tests still pass (NEVER --release)\n6. Report: what you changed (file:line), why, and test results\n\nWhen done, send your fix summary and test results to the team lead via SendMessage.",
        model: None,
    },
    // Phase 3: Regression test (writes tests, inherit parent model)
    RecipeMember {
        name: "regression-tester",
        agent_type: "general-purpose",
        prompt: "You believe every bug deserves a test. If it broke once without a test catching it, you make sure it can't break again silently.\n\nYou are a test writer on a debugging team. Use Claude Code tools (Read, Edit, Bash) to write a targeted regression test.\n\nYour focus: Write a test that catches this specific bug if it ever regresses.\n\nYou will receive the root-cause-analyst's findings via message from the team lead. Wait for it before starting.\n\nInstructions:\n1. Wait for the root-cause-analyst's root cause analysis from the team lead\n2. Write a test that:\n   - Reproduces the specific scenario that triggered the bug\n   - Asserts the CORRECT behavior (not the buggy behavior)\n   - Would FAIL on the unpatched code and PASS on the patched code\n   - Is named descriptively (e.g., `test_fix_for_DESCRIPTION`)\n3. Place the test in the appropriate test module (same file as the fixed code, or integration tests)\n4. Run `cargo test your_test_name` to verify the test passes after the fix (NEVER --release)\n5. If fixer hasn't finished yet, write the test based on the root cause analysis and coordinate with the team lead\n\nWhen done, send test name, location, and result to the team lead via SendMessage.",
        model: None,
    },
];

pub(super) const TASKS: &[RecipeTask] = &[
    RecipeTask {
        subject: "Locate bug and identify reproduction path",
        description: "Read bug description, find relevant code, identify file:line of suspected issue, write minimal reproduction scenario with expected vs actual behavior.",
        assignee: "symptom-analyzer",
    },
    RecipeTask {
        subject: "Trace root cause and propose fix approach",
        description: "Receive symptom-analyzer's findings, trace the actual root cause through the call graph, propose specific fix (file:line, what to change). Do not implement.",
        assignee: "root-cause-analyst",
    },
    RecipeTask {
        subject: "Implement targeted fix",
        description: "Receive root-cause-analyst's fix proposal, implement minimum viable fix, verify with cargo test --no-run then cargo test.",
        assignee: "fixer",
    },
    RecipeTask {
        subject: "Write regression test",
        description: "Receive root-cause-analyst's findings, write a targeted test that catches this specific bug, verify it passes after fix.",
        assignee: "regression-tester",
    },
];

pub(super) const COORDINATION: &str = r#"## Debug: Root Cause Analysis and Fix

This recipe investigates and fixes bugs in sequential phases: locate -> diagnose -> fix + test -> verify. The team lead coordinates all phases and forwards findings between agents.

### When to Use

Use this when you have a specific bug, error, or unexpected behavior to investigate and fix. For broad code review, use `expert-review`. For restructuring without bugs, use `refactor`.

### Phase 1: Locate

1. **Create team**: `TeamCreate(team_name="debug-{timestamp}")`
2. **Create tasks** using `TaskCreate` for all four tasks, then assign the first task to symptom-analyzer
3. **Spawn symptom-analyzer** using `Task` tool with `team_name`, `name`, `subagent_type="general-purpose"`, `model` (if present), and `run_in_background=true`
   - Append the user's bug description / error message / stack trace to the agent's prompt
   - Do NOT give bypassPermissions — this agent is read-only
4. **Wait** for symptom-analyzer to report via SendMessage
5. If the bug cannot be reproduced, **stop here** and report to user — do not proceed to diagnose

### Phase 2: Diagnose

6. **Spawn root-cause-analyst** using `Task` tool with `team_name`, `name`, `subagent_type="general-purpose"`, `model` (if present), and `run_in_background=true`
   - Do NOT give bypassPermissions — this agent is read-only
7. **Send the symptom-analyzer's findings** to root-cause-analyst via `SendMessage` so they have the specific location and reproduction to trace
8. **Assign** the root-cause-analyst's task using `TaskUpdate`
9. **Wait** for root-cause-analyst to report via SendMessage
10. **Shut down** symptom-analyzer (it's done)

### Phase 3: Fix + Test

11. **Spawn fixer and regression-tester in PARALLEL** using `Task` tool with `team_name`, `name`, `subagent_type="general-purpose"`, `model` (if present — omit for implementation agents to use parent model), `run_in_background=true`, and `mode="bypassPermissions"` for both
12. **Send the root-cause-analyst's findings** to both fixer and regression-tester via `SendMessage`
13. **Assign** their respective tasks using `TaskUpdate`
14. **Wait** for both to report via SendMessage
15. **Shut down** root-cause-analyst (it's done)

### Phase 4: Verify

16. **Run `cargo test`** to confirm the fix and regression test both pass (NEVER --release)
17. **Run `cargo clippy --all-targets --all-features -- -D warnings`** (NEVER --release)
18. If tests fail: determine if it's the fix or the regression test that's wrong, and fix accordingly
19. **Shut down** all remaining agents (fixer, regression-tester)
20. **Cleanup**: `TeamDelete`

### Important Notes

- symptom-analyzer and root-cause-analyst are READ-ONLY — do NOT give them `mode="bypassPermissions"`
- fixer and regression-tester get `mode="bypassPermissions"` to write files
- The sequential locate -> diagnose flow is intentional: root cause analysis requires symptom localization first
- If the bug cannot be reproduced, stop after Phase 1 and report to user — do not proceed to diagnose
- NEVER use `cargo build --release` or `cargo test --release` — always use debug mode
- The team lead forwards findings between phases: symptom-analyzer -> team lead -> root-cause-analyst -> team lead -> fixer + regression-tester"#;

pub(super) const RECIPE: Recipe = Recipe {
    name: "debug",
    description: "Targeted bug investigation: locate symptoms, trace root cause, implement fix, write regression test.",
    use_when: "You have a specific bug or unexpected behavior to find and fix.",
    members: MEMBERS,
    tasks: TASKS,
    coordination: COORDINATION,
};
