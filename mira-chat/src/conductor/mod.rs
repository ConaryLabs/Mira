//! Conductor - External state machine for DeepSeek orchestration
//!
//! The Conductor manages the two-brain architecture:
//! - Reasoner (64k output, no tools) → planning, analysis, diffs
//! - Chat (8k output, tools) → execution, tool calls
//!
//! Key principles:
//! - 12k context budget per turn
//! - Diff-only edits (no full file output)
//! - Deterministic escalation triggers
//! - External state management (providers are stateless)

mod ast_context;
mod config;
mod diff;
mod executor;
mod mira_intel;
mod observability;
mod orchestrator;
mod planning;
mod session;
mod state;
mod validation;

pub use ast_context::{ContextBuilder, FileOutline, Language, Symbol, SymbolKind};
pub use config::ConductorConfig;
pub use diff::{DiffError, DiffLine, Hunk, UnifiedDiff, generate_diff};
pub use mira_intel::{MiraIntel, FixSuggestion, RejectedApproach, CochangePattern};
pub use executor::{ExecutionStats, StepExecutor, StepResult, ToolCallRecord, ToolHandler};
pub use observability::{CostBreakdown, LatencyMetrics, SessionMetrics, StepMetrics, TokenUsage};
pub use orchestrator::{Conductor, ConductorError, CostTracker, TaskResult};
pub use planning::{parse_plan, Plan, PlanStep, StepType};
pub use session::{
    ConductorSession, SessionOptions, SessionResult,
    build_conductor_prompt, default_system_prompt, quick_session, session_with_context,
};
pub use state::{ConductorState, EscalationReason, ExecutionStep, StateTransition, StepStatus};
pub use validation::{repair_json, IssueSeverity, ToolSchemas, ValidationResult};
