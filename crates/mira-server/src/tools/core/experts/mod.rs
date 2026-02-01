// crates/mira-server/src/tools/core/experts/mod.rs
// Agentic expert sub-agents powered by LLM providers with tool access

use std::time::Duration;

mod config;
mod context;
mod council;
mod execution;
pub(crate) mod findings;
mod plan;
mod prompts;
mod role;
pub(crate) mod strategy;
mod tools;

// Re-export ToolContext from parent for use by submodules
pub(crate) use super::ToolContext;

// Constants used across the module
/// Maximum iterations for the agentic loop
pub(crate) const MAX_ITERATIONS: usize = 100;

/// Timeout for the entire expert consultation (including all tool calls)
pub(crate) const EXPERT_TIMEOUT: Duration = Duration::from_secs(600); // 10 minutes for multi-turn with reasoning models

/// Timeout for individual LLM calls (6 minutes for reasoning models like DeepSeek)
pub(crate) const LLM_CALL_TIMEOUT: Duration = Duration::from_secs(360);

/// Maximum concurrent expert consultations (prevents rate limit exhaustion)
pub(crate) const MAX_CONCURRENT_EXPERTS: usize = 3;

/// Timeout for parallel expert consultation (longer than single expert to allow queuing)
pub(crate) const PARALLEL_EXPERT_TIMEOUT: Duration = Duration::from_secs(900); // 15 minutes for reasoning models

// Public API exports
pub use config::configure_expert;
pub use execution::{consult_expert, consult_experts};
pub use findings::ParsedFinding;
pub use role::ExpertRole;

/// Unified expert tool dispatcher
pub async fn handle_expert<C: ToolContext + Clone + 'static>(
    ctx: &C,
    req: crate::mcp::requests::ExpertRequest,
) -> Result<String, String> {
    use crate::mcp::requests::ExpertAction;
    match req.action {
        ExpertAction::Consult => {
            let roles = req.roles.ok_or("roles is required for action 'consult'")?;
            let context = req
                .context
                .ok_or("context is required for action 'consult'")?;
            consult_experts(ctx, roles, context, req.question, req.mode).await
        }
        ExpertAction::Configure => {
            let config_action = req
                .config_action
                .ok_or("config_action is required for action 'configure'")?;
            configure_expert(ctx, config_action, req.role, req.prompt, req.provider, req.model)
                .await
        }
    }
}
