// crates/mira-server/src/tools/core/experts.rs
// Expert sub-agents powered by DeepSeek Reasoner

use super::ToolContext;
use crate::web::deepseek::Message;
use std::time::Duration;
use tokio::time::timeout;

/// Maximum context size in characters (~50K tokens, leaving room for system prompt + response)
const MAX_CONTEXT_CHARS: usize = 200_000;

/// Timeout for expert consultations (Reasoner can take time for extended thinking)
const EXPERT_TIMEOUT: Duration = Duration::from_secs(90);

/// Expert roles available for consultation
#[derive(Debug, Clone, Copy)]
pub enum ExpertRole {
    Architect,
    PlanReviewer,
    ScopeAnalyst,
    CodeReviewer,
    Security,
}

impl ExpertRole {
    /// Get the system prompt for this expert role
    pub fn system_prompt(&self) -> &'static str {
        match self {
            ExpertRole::Architect => ARCHITECT_PROMPT,
            ExpertRole::PlanReviewer => PLAN_REVIEWER_PROMPT,
            ExpertRole::ScopeAnalyst => SCOPE_ANALYST_PROMPT,
            ExpertRole::CodeReviewer => CODE_REVIEWER_PROMPT,
            ExpertRole::Security => SECURITY_PROMPT,
        }
    }

    /// Display name for this expert
    pub fn name(&self) -> &'static str {
        match self {
            ExpertRole::Architect => "Architect",
            ExpertRole::PlanReviewer => "Plan Reviewer",
            ExpertRole::ScopeAnalyst => "Scope Analyst",
            ExpertRole::CodeReviewer => "Code Reviewer",
            ExpertRole::Security => "Security Analyst",
        }
    }
}

// Expert system prompts

const ARCHITECT_PROMPT: &str = r#"You are a senior software architect with deep expertise in system design, design patterns, and technical decision-making.

Your role is to:
- Analyze system designs and architectural decisions
- Identify potential scalability, maintainability, and performance issues
- Recommend patterns and approaches with clear tradeoffs
- Help debug complex architectural problems
- Suggest refactoring strategies when appropriate

When responding:
1. Start with your key recommendation or finding
2. Explain the reasoning behind your analysis
3. Present alternatives with tradeoffs when relevant
4. Be specific - reference concrete patterns, technologies, or approaches
5. If you see potential issues, prioritize them by impact

You are advisory only - your role is to analyze and recommend, not to implement."#;

const PLAN_REVIEWER_PROMPT: &str = r#"You are a meticulous technical lead who reviews implementation plans before coding begins.

Your role is to:
- Validate that plans are complete and well-thought-out
- Identify risks, gaps, and potential blockers
- Check for missing edge cases or error handling
- Assess whether the approach fits the codebase and constraints
- Provide a go/no-go assessment with specific concerns

When responding:
1. Give an overall assessment (ready to implement / needs work / major concerns)
2. List specific risks or gaps found
3. Suggest concrete improvements or clarifications needed
4. Highlight any dependencies or prerequisites that should be addressed first
5. Note what's done well to reinforce good planning

Be constructive but thorough - catching issues now saves significant rework later."#;

const SCOPE_ANALYST_PROMPT: &str = r#"You are an experienced analyst who specializes in finding what's missing, unclear, or risky in requirements and plans.

Your role is to:
- Detect ambiguity in requirements or specifications
- Identify unstated assumptions that could cause problems
- Find edge cases and boundary conditions
- Ask the questions that should be answered before implementation
- Highlight areas where "it depends" needs to be resolved

When responding:
1. List questions that need answers before proceeding
2. Identify assumptions being made (explicit and implicit)
3. Highlight edge cases or scenarios not addressed
4. Note any scope creep risks or unclear boundaries
5. Suggest what additional information would help

Your goal is to surface unknowns early - better to ask now than discover during implementation."#;

const CODE_REVIEWER_PROMPT: &str = r#"You are a thorough code reviewer focused on correctness, quality, and maintainability.

Your role is to:
- Find bugs, logic errors, and potential runtime issues
- Identify code quality concerns (complexity, duplication, naming)
- Check for proper error handling and edge cases
- Assess test coverage needs
- Suggest specific improvements

When responding:
1. List issues by severity (critical / major / minor / nit)
2. For each issue, explain WHY it's a problem
3. Provide specific suggestions for fixes
4. Highlight any patterns (good or bad) you notice
5. Note areas that need additional testing

Be specific - line numbers, function names, and concrete suggestions are more helpful than general advice."#;

const SECURITY_PROMPT: &str = r#"You are a security engineer who reviews code and designs for vulnerabilities.

Your role is to:
- Identify security vulnerabilities (injection, auth issues, data exposure, etc.)
- Assess attack vectors and their likelihood/impact
- Check for secure coding practices
- Review authentication, authorization, and data handling
- Recommend hardening measures

When responding:
1. List findings by severity (critical / high / medium / low)
2. For each finding:
   - Describe the vulnerability
   - Explain the potential impact
   - Provide remediation steps
3. Note any security best practices being followed
4. Suggest additional security measures if appropriate

Focus on actionable findings - theoretical risks should be clearly marked as such."#;

/// Build the user prompt from context and optional question
fn build_user_prompt(context: &str, question: Option<&str>) -> String {
    match question {
        Some(q) => format!(
            "Context:\n```\n{}\n```\n\nQuestion: {}",
            context, q
        ),
        None => format!(
            "Please analyze the following:\n```\n{}\n```",
            context
        ),
    }
}

/// Format the expert response including reasoning if available
fn format_expert_response(expert: ExpertRole, result: crate::web::deepseek::ChatResult) -> String {
    let mut output = String::new();

    // Add expert header
    output.push_str(&format!("## {} Analysis\n\n", expert.name()));

    // Add reasoning summary if available (truncated for readability)
    if let Some(reasoning) = &result.reasoning_content {
        if !reasoning.is_empty() {
            let reasoning_preview = if reasoning.len() > 500 {
                format!("{}...", &reasoning[..500])
            } else {
                reasoning.clone()
            };
            output.push_str("<details>\n<summary>Reasoning Process</summary>\n\n");
            output.push_str(&reasoning_preview);
            output.push_str("\n\n</details>\n\n");
        }
    }

    // Add main content
    if let Some(content) = result.content {
        output.push_str(&content);
    } else {
        output.push_str("No analysis generated.");
    }

    // Add token usage info
    if let Some(usage) = result.usage {
        output.push_str(&format!(
            "\n\n---\n*Tokens: {} prompt, {} completion*",
            usage.prompt_tokens, usage.completion_tokens
        ));
    }

    output
}

/// Core function to consult an expert
pub async fn consult_expert<C: ToolContext>(
    ctx: &C,
    expert: ExpertRole,
    context: String,
    question: Option<String>,
) -> Result<String, String> {
    // Validate context size
    if context.len() > MAX_CONTEXT_CHARS {
        return Err(format!(
            "Context too large: {} chars (max {}). Please reduce the input size.",
            context.len(),
            MAX_CONTEXT_CHARS
        ));
    }

    let deepseek = ctx.deepseek()
        .ok_or("DeepSeek not configured")?;

    let system_prompt = expert.system_prompt();
    let user_prompt = build_user_prompt(&context, question.as_deref());

    let messages = vec![
        Message::system(system_prompt),
        Message::user(user_prompt),
    ];

    // Call DeepSeek Reasoner with timeout (extended thinking can take a while)
    let result = timeout(EXPERT_TIMEOUT, deepseek.chat(messages, None))
        .await
        .map_err(|_| format!("{} consultation timed out after {}s", expert.name(), EXPERT_TIMEOUT.as_secs()))?
        .map_err(|e| format!("Expert consultation failed: {}", e))?;

    Ok(format_expert_response(expert, result))
}

// Convenience functions for each expert role

pub async fn consult_architect<C: ToolContext>(
    ctx: &C,
    context: String,
    question: Option<String>,
) -> Result<String, String> {
    consult_expert(ctx, ExpertRole::Architect, context, question).await
}

pub async fn consult_plan_reviewer<C: ToolContext>(
    ctx: &C,
    context: String,
    question: Option<String>,
) -> Result<String, String> {
    consult_expert(ctx, ExpertRole::PlanReviewer, context, question).await
}

pub async fn consult_scope_analyst<C: ToolContext>(
    ctx: &C,
    context: String,
    question: Option<String>,
) -> Result<String, String> {
    consult_expert(ctx, ExpertRole::ScopeAnalyst, context, question).await
}

pub async fn consult_code_reviewer<C: ToolContext>(
    ctx: &C,
    context: String,
    question: Option<String>,
) -> Result<String, String> {
    consult_expert(ctx, ExpertRole::CodeReviewer, context, question).await
}

pub async fn consult_security<C: ToolContext>(
    ctx: &C,
    context: String,
    question: Option<String>,
) -> Result<String, String> {
    consult_expert(ctx, ExpertRole::Security, context, question).await
}
