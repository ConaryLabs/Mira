// crates/mira-server/src/tools/core/experts/role.rs
// ExpertRole enum and related implementations

use super::ToolContext;
use super::prompts::*;
use crate::llm::PromptBuilder;
use std::borrow::Cow;

/// Expert roles available for consultation
#[derive(Debug, Clone)]
pub enum ExpertRole {
    Architect,
    PlanReviewer,
    ScopeAnalyst,
    CodeReviewer,
    Security,
    /// Custom role with name and description
    Custom(String, String), // (name, description)
}

impl ExpertRole {
    /// Get the system prompt for this expert role (async to avoid blocking)
    /// Checks database for custom prompt first, falls back to default
    pub async fn system_prompt<C: ToolContext>(&self, ctx: &C) -> String {
        let role_key = self.db_key();

        // Get role instructions (custom or default) - use pool for async access
        let custom_prompt = ctx.pool().get_custom_prompt(&role_key).await.ok().flatten();

        let role_instructions = if let Some(prompt) = custom_prompt {
            prompt
        } else {
            match self {
                ExpertRole::Architect => ARCHITECT_PROMPT,
                ExpertRole::PlanReviewer => PLAN_REVIEWER_PROMPT,
                ExpertRole::ScopeAnalyst => SCOPE_ANALYST_PROMPT,
                ExpertRole::CodeReviewer => CODE_REVIEWER_PROMPT,
                ExpertRole::Security => SECURITY_PROMPT,
                ExpertRole::Custom(_name, description) => {
                    // For custom roles, build from the description
                    description
                }
            }
            .to_string()
        };

        // Build standardized prompt with static prefix and tool guidance
        // Include current date and MCP tools context
        let date_context = format!(
            "\n\nCurrent date: {}",
            chrono::Utc::now().format("%Y-%m-%d")
        );
        let mcp_context = super::context::get_mcp_tools_context(ctx).await;

        let base_prompt = PromptBuilder::new(role_instructions)
            .with_tool_guidance()
            .build_system_prompt();

        format!("{}{}{}", base_prompt, date_context, mcp_context)
    }

    /// Database key for this expert role.
    /// Avoids allocation for built-in roles by returning `Cow::Borrowed`.
    pub fn db_key(&self) -> Cow<'static, str> {
        match self {
            ExpertRole::Architect => "architect".into(),
            ExpertRole::PlanReviewer => "plan_reviewer".into(),
            ExpertRole::ScopeAnalyst => "scope_analyst".into(),
            ExpertRole::CodeReviewer => "code_reviewer".into(),
            ExpertRole::Security => "security".into(),
            ExpertRole::Custom(name, _) => {
                Cow::Owned(format!("custom:{}", name.to_lowercase().replace(' ', "_")))
            }
        }
    }

    /// Display name for this expert.
    /// Avoids allocation for built-in roles by returning `Cow::Borrowed`.
    pub fn name(&self) -> Cow<'static, str> {
        match self {
            ExpertRole::Architect => "Architect".into(),
            ExpertRole::PlanReviewer => "Plan Reviewer".into(),
            ExpertRole::ScopeAnalyst => "Scope Analyst".into(),
            ExpertRole::CodeReviewer => "Code Reviewer".into(),
            ExpertRole::Security => "Security Analyst".into(),
            ExpertRole::Custom(name, _) => Cow::Owned(name.clone()),
        }
    }

    /// Get role from database key (returns None for custom roles not in DB)
    pub fn from_db_key(key: &str) -> Option<Self> {
        match key {
            "architect" => Some(ExpertRole::Architect),
            "plan_reviewer" => Some(ExpertRole::PlanReviewer),
            "scope_analyst" => Some(ExpertRole::ScopeAnalyst),
            "code_reviewer" => Some(ExpertRole::CodeReviewer),
            "security" => Some(ExpertRole::Security),
            _ => {
                // Check for custom role pattern
                if let Some(rest) = key.strip_prefix("custom:") {
                    let name = rest.to_string();
                    Some(ExpertRole::Custom(
                        name.replace('_', " "),
                        "Custom expert role".to_string(),
                    ))
                } else {
                    None
                }
            }
        }
    }

    /// Create a custom role
    pub fn custom(name: String, description: String) -> Self {
        ExpertRole::Custom(name, description)
    }

    /// List all predefined roles (not custom ones)
    pub fn all() -> &'static [ExpertRole] {
        // Use a static array since we can't have static slice of non-Copy types
        // This is a workaround - in practice callers iterate over this
        static ROLES: &[ExpertRole] = &[
            ExpertRole::Architect,
            ExpertRole::PlanReviewer,
            ExpertRole::ScopeAnalyst,
            ExpertRole::CodeReviewer,
            ExpertRole::Security,
        ];
        ROLES
    }
}
