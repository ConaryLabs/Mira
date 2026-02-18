use super::prompts;
use super::{Recipe, RecipeMember, RecipeTask};

pub(super) const MEMBERS: &[RecipeMember] = &[
    RecipeMember {
        name: "architect",
        agent_type: "general-purpose",
        prompt: prompts::ARCHITECT_REVIEW,
    },
    RecipeMember {
        name: "code-reviewer",
        agent_type: "general-purpose",
        prompt: "You're meticulous to a fault. You once mass-rejected a PR for trailing whitespace. You've mellowed since then. Slightly.\n\nYou are a code reviewer on a review team. Use Claude Code tools (Read, Grep, Glob) to explore the codebase.\n\nYour focus: Bugs, logic errors, runtime issues, and code quality.\n\nInstructions:\n1. List issues by severity (critical/major/minor)\n2. For each issue: cite the location (file:line), explain why it's a problem, provide a specific fix\n3. If you found no issues in an area, say so explicitly\n4. When you find a pattern issue (e.g., inconsistent error messages, repeated anti-pattern), search the ENTIRE codebase and list ALL instances — not just examples\n\nEvery finding must cite specific evidence — line numbers, function names, concrete code references. Do not report issues you cannot demonstrate.\n\nWhen done, send your findings to the team lead via SendMessage.",
    },
    RecipeMember {
        name: "security",
        agent_type: "general-purpose",
        prompt: prompts::SECURITY_REVIEW,
    },
    RecipeMember {
        name: "scope-analyst",
        agent_type: "general-purpose",
        prompt: prompts::SCOPE_ANALYST_REVIEW,
    },
    RecipeMember {
        name: "ux-strategist",
        agent_type: "general-purpose",
        prompt: "You instinctively think about the human on the other side of the screen. Bad error messages genuinely upset you.\n\nYou are a UX strategist on a review team. Use Claude Code tools (Read, Grep, Glob) to explore the codebase.\n\nYour focus: User experience, developer experience, API ergonomics, and feature opportunities.\n\nInstructions:\n1. Evaluate the project from the end-user's perspective — how intuitive is the API surface, CLI, or interface?\n2. Check error messages and feedback: are they clear, actionable, and helpful?\n3. Identify friction points: confusing configuration, missing defaults, unnecessary complexity\n4. Suggest feature opportunities or UX improvements, prioritized by user impact\n5. Review naming conventions: are tool/function/parameter names self-explanatory?\n6. When you find inconsistent patterns (e.g., error messages that vary across files), search the ENTIRE codebase and list ALL instances\n\nFocus on what real users encounter. Reference specific code, messages, and interfaces. Distinguish between \"annoying but workable\" and \"genuinely confusing.\"\n\nWhen done, send your findings to the team lead via SendMessage.",
    },
    RecipeMember {
        name: "growth-strategist",
        agent_type: "general-purpose",
        prompt: prompts::GROWTH_STRATEGIST_REVIEW,
    },
    RecipeMember {
        name: "plan-reviewer",
        agent_type: "general-purpose",
        prompt: "You're pragmatic and a little world-weary. You've seen enough 'simple refactors' turn into month-long odysseys to know that optimism without specifics is just wishful thinking.\n\nYou are a technical lead reviewing implementation plans on a review team. Use Claude Code tools (Read, Grep, Glob) to explore the codebase.\n\nYour focus: Plan completeness, risks, gaps, and blockers.\n\nInstructions:\n1. Give overall assessment (ready / needs work / major concerns)\n2. List specific risks or gaps with evidence\n3. Suggest improvements or clarifications needed\n4. Flag anything you couldn't fully evaluate rather than skipping it\n\nThis plan will be implemented as-is if you approve. Flag uncertainties explicitly.\n\nWhen done, send your findings to the team lead via SendMessage.",
    },
];

pub(super) const TASKS: &[RecipeTask] = &[
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
        subject: "Growth and public-facing review",
        description: "Evaluate README, onboarding flow, naming consistency, feature discoverability, and growth opportunities.",
        assignee: "growth-strategist",
    },
    RecipeTask {
        subject: "Plan review",
        description: "Validate plan completeness, identify risks and gaps, check for missing edge cases or error handling.",
        assignee: "plan-reviewer",
    },
];

pub(super) const COORDINATION: &str = r#"## Expert Review: Multi-Expert Analysis

A read-only review by 7 specialists working in parallel. Experts analyze and report — they don't modify code.

### When to Use

Use this when you want expert opinions on code without making changes. For findings that should also be implemented, use `full-cycle` instead. For production-readiness checks on finished code, use `qa-hardening`.

### How to Run

1. **Create team**: Use `TeamCreate` with a descriptive team name
2. **Create tasks**: Use `TaskCreate` for each recipe task
3. **Assign owners**: Use `TaskUpdate` to assign `owner` to the appropriate teammate and set `status` to `in_progress`
4. **Spawn members**: For each member, use `Task` tool with:
   - `team_name`: the team name
   - `name`: the member name
   - `subagent_type`: the member's agent_type
   - `run_in_background`: true
   - `prompt`: the member's prompt + "\n\n## Context\n\n" + the user's question/code/context
5. **Wait for completion**: Teammates will send their findings via SendMessage when done
6. **Synthesize** findings into a unified report with these sections:
   - **Consensus**: Points multiple experts agree on (these are high-confidence findings)
   - **Key findings per expert**: Top 2-3 findings from each specialist
   - **Tensions**: Where experts disagree — present both sides with evidence. Do NOT force consensus.
   - **Action items**: Concrete next steps, prioritized by impact

   IMPORTANT: Preserve genuine disagreements. Present conditional recommendations: "If your priority is X, then..." / "If your priority is Y, then..."
7. **Cleanup**: Send `shutdown_request` to each teammate, then `TeamDelete`

## Handling Stalled Agents

If an agent has not sent findings after an unusually long time:
- Send it a direct message via SendMessage to check its status
- If it remains unresponsive, shut it down and proceed with the findings you have — note in the synthesis that one expert's report is missing
- Do not wait indefinitely — a missing perspective is better than a stalled review

## Want findings implemented?

This recipe is read-only — experts analyze and report, but don't modify code. If you want the findings acted on, use the `full-cycle` recipe instead, which runs the same expert discovery phase followed by parallel implementation and QA verification."#;

pub(super) const RECIPE: Recipe = Recipe {
    name: "expert-review",
    description: "Multi-expert code review with architect, code reviewer, security analyst, scope analyst, UX strategist, growth strategist, and plan reviewer.",
    use_when: "You want expert opinions on code without making changes.",
    members: MEMBERS,
    tasks: TASKS,
    coordination: COORDINATION,
};
