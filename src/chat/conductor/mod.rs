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
mod mira_intel;
mod observability;
mod planning;
mod state;
pub mod validation;

// Re-export public types
pub use ast_context::{ContextBuilder, FileOutline, Language, Symbol, SymbolKind};
pub use config::ConductorConfig;
pub use diff::{generate_diff, DiffError, DiffLine, Hunk, UnifiedDiff};
pub use mira_intel::{CochangePattern, FixSuggestion, MiraIntel, RejectedApproach};
pub use observability::{CostBreakdown, LatencyMetrics, ModelCost, SessionMetrics, StepMetrics, TokenUsage};
pub use planning::{parse_plan, Plan, PlanStep, StepType};
pub use state::{ConductorState, EscalationReason, ExecutionStep, StateTransition, StepStatus, TrackedToolCall};
pub use validation::{repair_json, IssueSeverity, ParamSchema, ParamType, ToolSchemas, ValidationResult, ValidationIssue};
