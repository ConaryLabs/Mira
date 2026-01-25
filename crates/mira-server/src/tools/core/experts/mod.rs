// crates/mira-server/src/tools/core/experts/mod.rs
// Agentic expert sub-agents powered by LLM providers with tool access

use std::time::Duration;

mod config;
mod context;
mod execution;
mod findings;
mod prompts;
mod role;
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
pub use execution::{
    consult_architect, consult_code_reviewer, consult_expert, consult_experts,
    consult_plan_reviewer, consult_scope_analyst, consult_security,
};
pub use findings::ParsedFinding;
pub use role::ExpertRole;
