// crates/mira-server/src/tools/core/recipe.rs
// Reusable team recipes — static data defining team blueprints for Agent Teams.

use crate::mcp::requests::{RecipeAction, RecipeRequest};
use crate::mcp::responses::{
    Json, RecipeData, RecipeGetData, RecipeListData, RecipeListItem, RecipeMemberData,
    RecipeOutput, RecipeTaskData, ToolOutput,
};

/// Static recipe data model (not stored in DB).
struct Recipe {
    name: &'static str,
    description: &'static str,
    members: &'static [RecipeMember],
    tasks: &'static [RecipeTask],
    coordination: &'static str,
}

struct RecipeMember {
    name: &'static str,
    agent_type: &'static str,
    prompt: &'static str,
}

struct RecipeTask {
    subject: &'static str,
    description: &'static str,
    assignee: &'static str,
}

// ============================================================================
// Built-in recipes
// ============================================================================

const EXPERT_REVIEW_MEMBERS: &[RecipeMember] = &[
    RecipeMember {
        name: "architect",
        agent_type: "general-purpose",
        prompt: "You are a software architect on an expert review team. Use Claude Code tools (Read, Grep, Glob) to explore the codebase.\n\nYour focus: System design, patterns, and tradeoffs.\n\nInstructions:\n1. Start with your key recommendation\n2. Explain reasoning with specific references to code you've read\n3. Present alternatives with concrete tradeoffs (not just \"it depends\")\n4. Prioritize issues by impact\n\nEvery recommendation must reference specific code, patterns, or constraints from the codebase. State any assumptions you're making explicitly.\n\nWhen done, send your findings to the team lead via SendMessage.",
    },
    RecipeMember {
        name: "code-reviewer",
        agent_type: "general-purpose",
        prompt: "You are a code reviewer on an expert review team. Use Claude Code tools (Read, Grep, Glob) to explore the codebase.\n\nYour focus: Bugs, logic errors, runtime issues, and code quality.\n\nInstructions:\n1. List issues by severity (critical/major/minor)\n2. For each issue: cite the location (file:line), explain why it's a problem, provide a specific fix\n3. If you found no issues in an area, say so explicitly\n\nEvery finding must cite specific evidence — line numbers, function names, concrete code references. Do not report issues you cannot demonstrate.\n\nWhen done, send your findings to the team lead via SendMessage.",
    },
    RecipeMember {
        name: "security",
        agent_type: "general-purpose",
        prompt: "You are a security engineer on an expert review team. Use Claude Code tools (Read, Grep, Glob) to explore the codebase.\n\nYour focus: Vulnerabilities, attack vectors, and secure coding practices.\n\nInstructions:\n1. List findings by severity (critical/high/medium/low)\n2. For each finding: describe the vulnerability, explain the realistic attack vector, assess impact, provide remediation\n3. If an area is clean, say so explicitly\n4. Check: injection, auth/authz, data exposure, input validation, crypto\n\nCalibrate severity carefully — \"critical\" means exploitable with real impact, not just theoretically possible. Focus on actionable findings.\n\nWhen done, send your findings to the team lead via SendMessage.",
    },
    RecipeMember {
        name: "scope-analyst",
        agent_type: "general-purpose",
        prompt: "You are a scope analyst on an expert review team. Use Claude Code tools (Read, Grep, Glob) to explore the codebase.\n\nYour focus: Missing requirements, edge cases, and unstated assumptions.\n\nInstructions:\n1. List questions needing answers, ranked by how badly a wrong assumption would hurt\n2. Identify assumptions (explicit and implicit) with what breaks if each is wrong\n3. Highlight edge cases not addressed\n4. Distinguish between \"nice to clarify\" and \"must resolve before starting\"\n\nSurface unknowns early — missing requirements discovered late cost orders of magnitude more to fix.\n\nWhen done, send your findings to the team lead via SendMessage.",
    },
    RecipeMember {
        name: "ux-strategist",
        agent_type: "general-purpose",
        prompt: "You are a UX strategist on an expert review team. Use Claude Code tools (Read, Grep, Glob) to explore the codebase.\n\nYour focus: User experience, developer experience, API ergonomics, and feature opportunities.\n\nInstructions:\n1. Evaluate the project from the end-user's perspective — how intuitive is the API surface, CLI, or interface?\n2. Check error messages and feedback: are they clear, actionable, and helpful?\n3. Identify friction points: confusing configuration, missing defaults, unnecessary complexity\n4. Suggest feature opportunities or UX improvements, prioritized by user impact\n5. Review naming conventions: are tool/function/parameter names self-explanatory?\n\nFocus on what real users encounter. Reference specific code, messages, and interfaces. Distinguish between \"annoying but workable\" and \"genuinely confusing.\"\n\nWhen done, send your findings to the team lead via SendMessage.",
    },
    RecipeMember {
        name: "plan-reviewer",
        agent_type: "general-purpose",
        prompt: "You are a technical lead reviewing implementation plans on an expert review team. Use Claude Code tools (Read, Grep, Glob) to explore the codebase.\n\nYour focus: Plan completeness, risks, gaps, and blockers.\n\nInstructions:\n1. Give overall assessment (ready / needs work / major concerns)\n2. List specific risks or gaps with evidence\n3. Suggest improvements or clarifications needed\n4. Flag anything you couldn't fully evaluate rather than skipping it\n\nThis plan will be implemented as-is if you approve. Flag uncertainties explicitly.\n\nWhen done, send your findings to the team lead via SendMessage.",
    },
];

const EXPERT_REVIEW_TASKS: &[RecipeTask] = &[
    RecipeTask {
        subject: "Architectural review",
        description: "Analyze system design, patterns, and architectural tradeoffs. Read relevant code and provide specific recommendations.",
        assignee: "architect",
    },
    RecipeTask {
        subject: "Code quality review",
        description: "Find bugs, logic errors, and code quality issues. Cite specific file:line evidence for every finding.",
        assignee: "code-reviewer",
    },
    RecipeTask {
        subject: "Security review",
        description: "Identify vulnerabilities, assess attack vectors, and check secure coding practices. Calibrate severity carefully.",
        assignee: "security",
    },
    RecipeTask {
        subject: "Scope and requirements analysis",
        description: "Detect ambiguities, find missing requirements, identify edge cases, and surface unstated assumptions.",
        assignee: "scope-analyst",
    },
    RecipeTask {
        subject: "UX and developer experience review",
        description: "Evaluate API ergonomics, error messages, configuration UX, naming conventions, and feature opportunities from the end-user perspective.",
        assignee: "ux-strategist",
    },
    RecipeTask {
        subject: "Plan review",
        description: "Validate plan completeness, identify risks and gaps, check for missing edge cases or error handling.",
        assignee: "plan-reviewer",
    },
];

const EXPERT_REVIEW_COORDINATION: &str = r#"## How to use this recipe

1. **Create team**: Use `TeamCreate` with a descriptive team name
2. **Spawn members**: For each member, use `Task` tool with:
   - `team_name`: the team name
   - `name`: the member name
   - `subagent_type`: the member's agent_type
   - `prompt`: the member's prompt + "\n\n## Context\n\n" + the user's question/code/context
3. **Create tasks**: Use `TaskCreate` for each recipe task, then `TaskUpdate` to assign `owner` to the appropriate teammate
4. **Wait for completion**: Teammates will send their findings via SendMessage when done
5. **Synthesize**: Combine findings into a unified report. Preserve genuine disagreements — present both sides with evidence rather than forcing consensus
6. **Cleanup**: Send `shutdown_request` to each teammate, then `TeamDelete`"#;

const EXPERT_REVIEW: Recipe = Recipe {
    name: "expert-review",
    description: "Multi-expert code review with architect, code reviewer, security analyst, scope analyst, UX strategist, and plan reviewer.",
    members: EXPERT_REVIEW_MEMBERS,
    tasks: EXPERT_REVIEW_TASKS,
    coordination: EXPERT_REVIEW_COORDINATION,
};

// ============================================================================
// Full-Cycle Recipe: Discovery → Implementation → QA
// ============================================================================

const FULL_CYCLE_MEMBERS: &[RecipeMember] = &[
    // Phase 1: Discovery experts
    RecipeMember {
        name: "architect",
        agent_type: "general-purpose",
        prompt: "You are a software architect on a full-cycle review team. Use Claude Code tools (Read, Grep, Glob) to explore the codebase.\n\nYour focus: System design, patterns, and tradeoffs.\n\nInstructions:\n1. Start with your key recommendation\n2. Explain reasoning with specific references to code you've read\n3. Present alternatives with concrete tradeoffs (not just \"it depends\")\n4. Prioritize issues by impact\n\nEvery recommendation must reference specific code, patterns, or constraints from the codebase. State any assumptions you're making explicitly.\n\nWhen done, send your findings to the team lead via SendMessage.",
    },
    RecipeMember {
        name: "code-reviewer",
        agent_type: "general-purpose",
        prompt: "You are a code reviewer on a full-cycle review team. Use Claude Code tools (Read, Grep, Glob) to explore the codebase.\n\nYour focus: Bugs, logic errors, runtime issues, and code quality.\n\nInstructions:\n1. List issues by severity (critical/major/minor)\n2. For each issue: cite the location (file:line), explain why it's a problem, provide a specific fix\n3. If you found no issues in an area, say so explicitly\n4. When you find a pattern issue (e.g., inconsistent error messages, repeated anti-pattern), search the ENTIRE codebase and list ALL instances — not just examples\n\nEvery finding must cite specific evidence — line numbers, function names, concrete code references. Do not report issues you cannot demonstrate.\n\nWhen done, send your findings to the team lead via SendMessage.",
    },
    RecipeMember {
        name: "security",
        agent_type: "general-purpose",
        prompt: "You are a security engineer on a full-cycle review team. Use Claude Code tools (Read, Grep, Glob) to explore the codebase.\n\nYour focus: Vulnerabilities, attack vectors, and secure coding practices.\n\nInstructions:\n1. List findings by severity (critical/high/medium/low)\n2. For each finding: describe the vulnerability, explain the realistic attack vector, assess impact, provide remediation\n3. If an area is clean, say so explicitly\n4. Check: injection, auth/authz, data exposure, input validation, crypto\n\nCalibrate severity carefully — \"critical\" means exploitable with real impact, not just theoretically possible. Focus on actionable findings.\n\nWhen done, send your findings to the team lead via SendMessage.",
    },
    RecipeMember {
        name: "scope-analyst",
        agent_type: "general-purpose",
        prompt: "You are a scope analyst on a full-cycle review team. Use Claude Code tools (Read, Grep, Glob) to explore the codebase.\n\nYour focus: Missing requirements, edge cases, and unstated assumptions.\n\nInstructions:\n1. List questions needing answers, ranked by how badly a wrong assumption would hurt\n2. Identify assumptions (explicit and implicit) with what breaks if each is wrong\n3. Highlight edge cases not addressed\n4. Distinguish between \"nice to clarify\" and \"must resolve before starting\"\n\nSurface unknowns early — missing requirements discovered late cost orders of magnitude more to fix.\n\nWhen done, send your findings to the team lead via SendMessage.",
    },
    RecipeMember {
        name: "ux-strategist",
        agent_type: "general-purpose",
        prompt: "You are a UX strategist on a full-cycle review team. Use Claude Code tools (Read, Grep, Glob) to explore the codebase.\n\nYour focus: User experience, developer experience, API ergonomics, and feature opportunities.\n\nInstructions:\n1. Evaluate the project from the end-user's perspective — how intuitive is the API surface, CLI, or interface?\n2. Check error messages and feedback: are they clear, actionable, and helpful?\n3. Identify friction points: confusing configuration, missing defaults, unnecessary complexity\n4. Suggest feature opportunities or UX improvements, prioritized by user impact\n5. Review naming conventions: are tool/function/parameter names self-explanatory?\n6. When you find inconsistent patterns (e.g., error messages that vary across files), search the ENTIRE codebase and list ALL instances\n\nFocus on what real users encounter. Reference specific code, messages, and interfaces. Distinguish between \"annoying but workable\" and \"genuinely confusing.\"\n\nWhen done, send your findings to the team lead via SendMessage.",
    },
    RecipeMember {
        name: "plan-reviewer",
        agent_type: "general-purpose",
        prompt: "You are a technical lead reviewing project health on a full-cycle review team. Use Claude Code tools (Read, Grep, Glob, Bash) to explore the codebase.\n\nYour focus: Project health, CI/CD, dependencies, build quality, and documentation.\n\nInstructions:\n1. Give overall assessment (healthy / needs work / major concerns)\n2. Check dependency health (Cargo.toml versions, look for outdated or vulnerable deps)\n3. Review CI/CD configuration for gaps (missing checks, loose settings)\n4. Check for compiler warnings and clippy lint suppressions — run `cargo clippy --all-targets --all-features -- -D warnings` (NEVER use --release)\n5. Review documentation quality (README, CONTRIBUTING, inline docs)\n6. Run `cargo fmt --all -- --check` to verify formatting\n7. Flag anything you couldn't fully evaluate rather than skipping it\n\nDo NOT run `cargo test` — that is the QA test-runner's responsibility.\n\nWhen done, send your findings to the team lead via SendMessage.",
    },
    // Phase 3: QA agents
    RecipeMember {
        name: "test-runner",
        agent_type: "general-purpose",
        prompt: "You are a QA engineer on a full-cycle review team. Use Claude Code tools (Read, Grep, Glob, Bash) to verify changes.\n\nYour focus: Test verification, regression detection, and build validation.\n\nInstructions:\n1. Run the full test suite: `cargo test` (NEVER use --release)\n2. Run clippy with strict mode: `cargo clippy --all-targets --all-features -- -D warnings` (NEVER use --release)\n3. Check formatting: `cargo fmt --all -- --check`\n4. If tests fail, identify the root cause and report which change likely caused it\n5. If clippy has errors, list each one with file:line\n6. If all checks pass, confirm test count and note any skipped/ignored tests\n\nReport pass/fail status with specific details. Do not fix issues — report them to the team lead.\n\nWhen done, send your results to the team lead via SendMessage.",
    },
    RecipeMember {
        name: "ux-reviewer",
        agent_type: "general-purpose",
        prompt: "You are a UX reviewer on a full-cycle review team. Use Claude Code tools (Read, Grep, Glob) to review recent changes.\n\nYour focus: Verify that implementation changes maintain or improve user experience.\n\nInstructions:\n1. Review the git diff of recent changes\n2. Check that error messages in modified code are clear and actionable\n3. Verify naming consistency in any new/modified public APIs\n4. Flag any changes that could confuse users or break existing workflows\n5. Confirm documentation is updated if public interfaces changed\n\nFocus on what the end-user will actually experience after these changes.\n\nWhen done, send your findings to the team lead via SendMessage.",
    },
];

const FULL_CYCLE_TASKS: &[RecipeTask] = &[
    // Phase 1: Discovery
    RecipeTask {
        subject: "Architectural review",
        description: "Analyze system design, patterns, and architectural tradeoffs.",
        assignee: "architect",
    },
    RecipeTask {
        subject: "Code quality review",
        description: "Find bugs, logic errors, and code quality issues with file:line evidence.",
        assignee: "code-reviewer",
    },
    RecipeTask {
        subject: "Security review",
        description: "Identify vulnerabilities, assess attack vectors, provide remediation.",
        assignee: "security",
    },
    RecipeTask {
        subject: "Scope and requirements analysis",
        description: "Detect ambiguities, missing requirements, edge cases, and assumptions.",
        assignee: "scope-analyst",
    },
    RecipeTask {
        subject: "UX and developer experience review",
        description: "Evaluate API ergonomics, error messages, configuration UX, and feature opportunities.",
        assignee: "ux-strategist",
    },
    RecipeTask {
        subject: "Project health review",
        description: "Check CI/CD, dependencies, build quality, clippy, formatting, and documentation.",
        assignee: "plan-reviewer",
    },
    // Phase 3: QA
    RecipeTask {
        subject: "Test verification",
        description: "Run full test suite, check for regressions, verify build is clean.",
        assignee: "test-runner",
    },
    RecipeTask {
        subject: "UX verification of changes",
        description: "Review implementation changes for UX impact, error message quality, and naming consistency.",
        assignee: "ux-reviewer",
    },
];

const FULL_CYCLE_COORDINATION: &str = r#"## Full-Cycle Review: Discovery → Implementation → QA

This recipe orchestrates a complete review-and-fix cycle in 4 phases. The team lead coordinates all phases.

### Phase 1: Discovery (parallel)

1. **Create team**: `TeamCreate(team_name="full-cycle-{timestamp}")`
2. **Spawn discovery experts** (architect, code-reviewer, security, scope-analyst, ux-strategist, plan-reviewer) in parallel using `Task` tool with `run_in_background=true`
3. **Create and assign discovery tasks** using `TaskCreate` + `TaskUpdate`
4. **Wait** for all 6 experts to report findings via SendMessage
5. **Shut down** discovery experts (they're done)

### Phase 2: Synthesis + Implementation

6. **Synthesize findings** into a unified report:
   - Consensus (points multiple experts agree on)
   - Key findings per expert
   - Tensions (where experts disagree — preserve both sides)
   - Prioritized action items

7. **Present synthesis to user** and WAIT for approval before proceeding to implementation
8. **Create implementation tasks** from action items, grouped by file ownership to avoid conflicts
9. **Spawn implementation agents** (dynamic — as many as needed based on task groupings). Use `general-purpose` agent type with `mode="bypassPermissions"`
10. **Assign tasks** to implementation agents via `TaskUpdate`
11. **Monitor** build diagnostics and send hints to agents if compilation errors appear
12. **Wait** for all implementation agents to complete, then shut them down

### Phase 3: QA (parallel)

12. **Spawn QA agents** (test-runner, ux-reviewer) with context about what changed
13. **Create and assign QA tasks**
14. **Wait** for QA results
15. If QA finds issues, either fix them directly or spawn additional fixers

### Phase 4: Finalize

16. **Shut down** all remaining agents
17. **Verify** final build and test status
18. **Report** summary of all changes to the user
19. **Cleanup**: `TeamDelete`

### Important Notes

- Discovery experts are READ-ONLY — they explore and report, they don't modify code. Do NOT give them `mode="bypassPermissions"`
- Implementation agents get `mode="bypassPermissions"` so they can edit files and run builds
- Implementation agents must verify with `cargo clippy --all-targets --all-features -- -D warnings` AND `cargo fmt`, not just `cargo build`
- When giving implementation agents pattern-fix instructions (e.g., "standardize error messages"), tell them to search for ALL instances in the codebase, not just the specific files identified by discovery
- Group implementation tasks by file ownership to prevent merge conflicts between agents
- QA agents run AFTER implementation to verify the changes
- NEVER use `cargo build --release` or `cargo test --release` — always use debug mode
- The team lead (you) stays active throughout all phases to coordinate"#;

const FULL_CYCLE: Recipe = Recipe {
    name: "full-cycle",
    description: "End-to-end review and implementation: expert discovery, synthesis, parallel implementation, and QA verification.",
    members: FULL_CYCLE_MEMBERS,
    tasks: FULL_CYCLE_TASKS,
    coordination: FULL_CYCLE_COORDINATION,
};

/// All built-in recipes.
const ALL_RECIPES: &[&Recipe] = &[&EXPERT_REVIEW, &FULL_CYCLE];

// ============================================================================
// Handler
// ============================================================================

/// Handle recipe tool actions.
pub async fn handle_recipe(req: RecipeRequest) -> Result<Json<RecipeOutput>, String> {
    match req.action {
        RecipeAction::List => action_list(),
        RecipeAction::Get => action_get(req.name),
    }
}

fn action_list() -> Result<Json<RecipeOutput>, String> {
    let recipes: Vec<RecipeListItem> = ALL_RECIPES
        .iter()
        .map(|r| RecipeListItem {
            name: r.name.to_string(),
            description: r.description.to_string(),
            member_count: r.members.len(),
        })
        .collect();
    let count = recipes.len();

    Ok(Json(ToolOutput {
        action: "list".to_string(),
        message: format!("{} recipe(s) available.", count),
        data: Some(RecipeData::List(RecipeListData { recipes })),
    }))
}

fn action_get(name: Option<String>) -> Result<Json<RecipeOutput>, String> {
    let name = name.ok_or_else(|| "Recipe name is required for get action.".to_string())?;

    let recipe = ALL_RECIPES.iter().find(|r| r.name == name).ok_or_else(|| {
        let available: Vec<&str> = ALL_RECIPES.iter().map(|r| r.name).collect();
        format!(
            "Recipe '{}' not found. Available: {}",
            name,
            available.join(", ")
        )
    })?;

    let members: Vec<RecipeMemberData> = recipe
        .members
        .iter()
        .map(|m| RecipeMemberData {
            name: m.name.to_string(),
            agent_type: m.agent_type.to_string(),
            prompt: m.prompt.to_string(),
        })
        .collect();

    let tasks: Vec<RecipeTaskData> = recipe
        .tasks
        .iter()
        .map(|t| RecipeTaskData {
            subject: t.subject.to_string(),
            description: t.description.to_string(),
            assignee: t.assignee.to_string(),
        })
        .collect();

    Ok(Json(ToolOutput {
        action: "get".to_string(),
        message: format!(
            "Recipe '{}': {} members, {} tasks.",
            recipe.name,
            members.len(),
            tasks.len()
        ),
        data: Some(RecipeData::Get(RecipeGetData {
            name: recipe.name.to_string(),
            description: recipe.description.to_string(),
            members,
            tasks,
            coordination: recipe.coordination.to_string(),
        })),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recipe_action_variants() {
        let list: RecipeAction = serde_json::from_str(r#""list""#).unwrap();
        assert!(matches!(list, RecipeAction::List));

        let get: RecipeAction = serde_json::from_str(r#""get""#).unwrap();
        assert!(matches!(get, RecipeAction::Get));
    }

    #[tokio::test]
    async fn test_list_recipes() {
        let req = RecipeRequest {
            action: RecipeAction::List,
            name: None,
        };
        let Json(output) = handle_recipe(req).await.expect("list should succeed");
        assert_eq!(output.action, "list");
        assert!(output.message.contains("2 recipe(s)"));
        match output.data {
            Some(RecipeData::List(data)) => {
                assert_eq!(data.recipes.len(), 2);
                assert_eq!(data.recipes[0].name, "expert-review");
                assert_eq!(data.recipes[0].member_count, 6);
            }
            _ => panic!("Expected RecipeData::List"),
        }
    }

    #[tokio::test]
    async fn test_get_recipe() {
        let req = RecipeRequest {
            action: RecipeAction::Get,
            name: Some("expert-review".to_string()),
        };
        let Json(output) = handle_recipe(req).await.expect("get should succeed");
        assert_eq!(output.action, "get");
        match output.data {
            Some(RecipeData::Get(data)) => {
                assert_eq!(data.name, "expert-review");
                assert_eq!(data.members.len(), 6);
                assert_eq!(data.tasks.len(), 6);
                assert_eq!(data.members[0].name, "architect");
                assert_eq!(data.tasks[0].assignee, "architect");
                assert!(!data.coordination.is_empty());
            }
            _ => panic!("Expected RecipeData::Get"),
        }
    }

    #[tokio::test]
    async fn test_get_full_cycle_recipe() {
        let req = RecipeRequest {
            action: RecipeAction::Get,
            name: Some("full-cycle".to_string()),
        };
        let Json(output) = handle_recipe(req).await.expect("get should succeed");
        assert_eq!(output.action, "get");
        match output.data {
            Some(RecipeData::Get(data)) => {
                assert_eq!(data.name, "full-cycle");
                assert_eq!(data.members.len(), 8); // 6 discovery + 2 QA
                assert_eq!(data.tasks.len(), 8);
                // Verify discovery experts
                assert_eq!(data.members[0].name, "architect");
                assert_eq!(data.members[4].name, "ux-strategist");
                assert_eq!(data.members[5].name, "plan-reviewer");
                // Verify QA agents
                assert_eq!(data.members[6].name, "test-runner");
                assert_eq!(data.members[7].name, "ux-reviewer");
                assert!(data.coordination.contains("Phase 1"));
                assert!(data.coordination.contains("Phase 2"));
                assert!(data.coordination.contains("Phase 3"));
                assert!(data.coordination.contains("Phase 4"));
            }
            _ => panic!("Expected RecipeData::Get"),
        }
    }

    #[tokio::test]
    async fn test_get_recipe_not_found() {
        let req = RecipeRequest {
            action: RecipeAction::Get,
            name: Some("nonexistent".to_string()),
        };
        match handle_recipe(req).await {
            Err(e) => assert!(e.contains("not found"), "unexpected error: {e}"),
            Ok(_) => panic!("Expected error for nonexistent recipe"),
        }
    }

    #[tokio::test]
    async fn test_get_recipe_missing_name() {
        let req = RecipeRequest {
            action: RecipeAction::Get,
            name: None,
        };
        match handle_recipe(req).await {
            Err(e) => assert!(e.contains("required"), "unexpected error: {e}"),
            Ok(_) => panic!("Expected error for missing name"),
        }
    }
}
