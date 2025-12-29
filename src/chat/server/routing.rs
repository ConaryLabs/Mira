//! Model routing for Gemini Flash/Pro selection
//!
//! DISABLED: Pro escalation is disabled due to Gemini 3 Pro rate limits.
//! All requests now use Flash only until quotas reset.
//!
//! Previous behavior (disabled):
//! 1. Start with model based on task type (Debugging/Planning → Pro, others → Flash)
//! 2. Escalate to Pro when heavy tools are requested
//! 3. Escalate to Pro when tool chain exceeds depth threshold

use crate::chat::provider::GeminiModel;
use crate::orchestrator::TaskType;

// ============================================================================
// Tool Categories
// ============================================================================

// NOTE: Pro escalation is DISABLED. Flash handles everything.
// These constants are kept for reference but not used.
#[allow(dead_code)]
const HEAVY_TOOLS: &[&str] = &["send_instruction"];
#[allow(dead_code)]
const ESCALATION_CHAIN_DEPTH: usize = 8;

// ============================================================================
// Routing Logic
// ============================================================================

/// Check if a tool requires Pro model for optimal results
/// DISABLED: Always returns false to force Flash usage due to Pro rate limits
pub fn requires_pro(_tool_name: &str) -> bool {
    // DISABLED: Pro escalation disabled due to rate limits
    // Original: HEAVY_TOOLS.contains(&tool_name)
    false
}

/// Routing state to track escalation decisions
#[derive(Debug, Clone)]
pub struct RoutingState {
    /// Current model selection
    pub current_model: GeminiModel,
    /// Cumulative tool calls in this turn
    pub tool_call_count: usize,
    /// Whether we've escalated (one-way: never de-escalate)
    pub has_escalated: bool,
    /// Reason for last escalation (for logging)
    pub escalation_reason: Option<String>,
    /// Current task type (for context-aware decisions)
    pub task_type: Option<TaskType>,
    /// Recommended thinking level based on task
    pub thinking_level: &'static str,
}

impl Default for RoutingState {
    fn default() -> Self {
        Self {
            current_model: GeminiModel::Flash,
            tool_call_count: 0,
            has_escalated: false,
            escalation_reason: None,
            task_type: None,
            thinking_level: "minimal",
        }
    }
}

impl RoutingState {
    /// Create new routing state starting with Flash
    pub fn new() -> Self {
        Self::default()
    }

    /// Create routing state with task-aware model selection
    ///
    /// DISABLED: Pro escalation disabled due to rate limits.
    /// All tasks now use Flash regardless of type.
    ///
    /// Previous behavior (disabled):
    /// - Debugging: Complex problem solving → Pro
    /// - Planning: Multi-step goal orchestration → Pro
    pub fn with_task_type(task_type: TaskType) -> Self {
        // DISABLED: Always use Flash due to Pro rate limits
        // Original logic:
        // let model = match task_type.recommended_model() {
        //     "pro" => GeminiModel::Pro,
        //     _ => GeminiModel::Flash,
        // };

        Self {
            current_model: GeminiModel::Flash,  // Always Flash
            tool_call_count: 0,
            has_escalated: false,  // Never escalate
            escalation_reason: None,
            task_type: Some(task_type),
            thinking_level: task_type.recommended_thinking_level(),
        }
    }

    /// Update task type and adjust thinking level (model stays Flash)
    pub fn set_task_type(&mut self, task_type: TaskType) {
        self.task_type = Some(task_type);
        self.thinking_level = task_type.recommended_thinking_level();
        // Pro escalation disabled - stay on Flash
    }

    /// Get the current thinking level (task-aware or default)
    pub fn get_thinking_level(&self, tool_count: usize) -> &'static str {
        // If we have a task-specific level, use it
        if self.task_type.is_some() {
            return self.thinking_level;
        }

        // Default behavior based on model and tool count
        self.current_model.select_thinking_level(tool_count > 0, tool_count)
    }

    /// Process tool calls and determine if escalation is needed.
    /// Returns true if model changed.
    ///
    /// DISABLED: Pro escalation completely disabled - Flash handles everything.
    pub fn process_tool_calls(&mut self, tool_names: &[&str]) -> bool {
        self.tool_call_count += tool_names.len();
        // Pro escalation disabled - always stay on Flash
        false
    }

    /// Force escalation to Pro (DISABLED - kept for future use)
    #[allow(dead_code)]
    fn escalate(&mut self, reason: String) {
        tracing::info!("Escalating Flash → Pro: {}", reason);
        self.current_model = GeminiModel::Pro;
        self.has_escalated = true;
        self.escalation_reason = Some(reason);
    }

    /// Get the current model
    pub fn model(&self) -> GeminiModel {
        self.current_model
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pro_escalation_disabled() {
        // Pro escalation is disabled - all tools should stay on Flash
        assert!(!requires_pro("goal"));
        assert!(!requires_pro("task"));
        assert!(!requires_pro("send_instruction"));
        assert!(!requires_pro("anything"));
    }

    #[test]
    fn test_routing_always_flash() {
        let state = RoutingState::new();
        assert_eq!(state.current_model, GeminiModel::Flash);
        assert!(!state.has_escalated);
    }

    #[test]
    fn test_all_tools_stay_on_flash() {
        let mut state = RoutingState::new();

        // All tools stay on Flash - Pro escalation completely disabled
        let changed = state.process_tool_calls(&["goal"]);
        assert!(!changed);
        assert_eq!(state.current_model, GeminiModel::Flash);

        let changed = state.process_tool_calls(&["task"]);
        assert!(!changed);
        assert_eq!(state.current_model, GeminiModel::Flash);

        let changed = state.process_tool_calls(&["send_instruction"]);
        assert!(!changed);
        assert_eq!(state.current_model, GeminiModel::Flash);

        // Even exceeding chain depth stays on Flash
        for _ in 0..20 {
            let changed = state.process_tool_calls(&["any_tool"]);
            assert!(!changed);
        }
        assert_eq!(state.current_model, GeminiModel::Flash);
    }

    #[test]
    fn test_all_task_types_use_flash() {
        // All task types use Flash - Pro escalation disabled
        let state = RoutingState::with_task_type(TaskType::Debugging);
        assert_eq!(state.current_model, GeminiModel::Flash);
        assert!(!state.has_escalated);

        let state = RoutingState::with_task_type(TaskType::Planning);
        assert_eq!(state.current_model, GeminiModel::Flash);
        assert!(!state.has_escalated);

        let state = RoutingState::with_task_type(TaskType::Exploration);
        assert_eq!(state.current_model, GeminiModel::Flash);
        assert!(!state.has_escalated);

        let state = RoutingState::with_task_type(TaskType::NewFeature);
        assert_eq!(state.current_model, GeminiModel::Flash);
        assert!(!state.has_escalated);
    }

    #[test]
    fn test_thinking_level_preserved() {
        // Even though all use Flash, thinking levels vary by task type
        let state = RoutingState::with_task_type(TaskType::Debugging);
        assert_eq!(state.thinking_level, "low");

        let state = RoutingState::with_task_type(TaskType::Exploration);
        assert_eq!(state.thinking_level, "minimal");

        let state = RoutingState::with_task_type(TaskType::Research);
        assert_eq!(state.thinking_level, "medium");
    }
}
