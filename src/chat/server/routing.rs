//! Model routing for Gemini Flash/Pro selection
//!
//! Task-aware escalation with tool-gated fallback:
//! 1. Start with model based on task type (Debugging/Planning → Pro, others → Flash)
//! 2. Escalate to Pro when heavy tools are requested
//! 3. Escalate to Pro when tool chain exceeds depth threshold

use crate::chat::provider::GeminiModel;
use crate::orchestrator::TaskType;

// ============================================================================
// Tool Categories
// ============================================================================

/// Heavy tools that benefit from Pro's advanced reasoning.
/// These involve complex decision-making and multi-step planning.
const HEAVY_TOOLS: &[&str] = &[
    // Planning and project management
    "goal",
    "task",
    "correction",
    // Orchestration (sending work to Claude Code)
    "send_instruction",
];

/// Light tools that Flash handles efficiently.
/// Simple queries, lookups, and basic operations.
#[allow(dead_code)]
const LIGHT_TOOLS: &[&str] = &[
    // Simple orchestration (read-only)
    "view_claude_activity",
    "list_instructions",
    "cancel_instruction",
    // Simple storage
    "store_decision",
    "record_rejected_approach",
    // Memory operations (would be handled by Mira MCP, but listed for reference)
    "remember",
    "recall",
];

/// Maximum tool chain depth before escalating to Pro.
/// When cumulative tool calls exceed this, switch to Pro for better reasoning.
const ESCALATION_CHAIN_DEPTH: usize = 3;

// ============================================================================
// Routing Logic
// ============================================================================

/// Check if a tool requires Pro model for optimal results
pub fn requires_pro(tool_name: &str) -> bool {
    HEAVY_TOOLS.contains(&tool_name)
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
    /// Task types that benefit from Pro's advanced reasoning:
    /// - Debugging: Complex problem solving, root cause analysis
    /// - Planning: Multi-step goal orchestration
    pub fn with_task_type(task_type: TaskType) -> Self {
        let model = match task_type.recommended_model() {
            "pro" => GeminiModel::Pro,
            _ => GeminiModel::Flash,
        };

        let has_escalated = model == GeminiModel::Pro;
        let escalation_reason = if has_escalated {
            Some(format!("task type: {}", task_type.as_str()))
        } else {
            None
        };

        Self {
            current_model: model,
            tool_call_count: 0,
            has_escalated,
            escalation_reason,
            task_type: Some(task_type),
            thinking_level: task_type.recommended_thinking_level(),
        }
    }

    /// Update task type and potentially adjust model
    pub fn set_task_type(&mut self, task_type: TaskType) {
        self.task_type = Some(task_type);
        self.thinking_level = task_type.recommended_thinking_level();

        // Only escalate, never de-escalate
        if !self.has_escalated && task_type.recommended_model() == "pro" {
            self.escalate(format!("task type: {}", task_type.as_str()));
        }
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
    pub fn process_tool_calls(&mut self, tool_names: &[&str]) -> bool {
        if self.has_escalated {
            // Already on Pro, no further escalation needed
            self.tool_call_count += tool_names.len();
            return false;
        }

        self.tool_call_count += tool_names.len();

        // Check 1: Any heavy tool requested?
        for name in tool_names {
            if requires_pro(name) {
                self.escalate(format!("heavy tool: {}", name));
                return true;
            }
        }

        // Check 2: Tool chain depth exceeded?
        if self.tool_call_count > ESCALATION_CHAIN_DEPTH {
            self.escalate(format!(
                "chain depth {} > {}",
                self.tool_call_count, ESCALATION_CHAIN_DEPTH
            ));
            return true;
        }

        false
    }

    /// Force escalation to Pro
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
    fn test_heavy_tool_detection() {
        assert!(requires_pro("goal"));
        assert!(requires_pro("task"));
        assert!(requires_pro("send_instruction"));

        assert!(!requires_pro("view_claude_activity"));
        assert!(!requires_pro("list_instructions"));
        assert!(!requires_pro("remember"));
    }

    #[test]
    fn test_routing_starts_with_flash() {
        let state = RoutingState::new();
        assert_eq!(state.current_model, GeminiModel::Flash);
        assert!(!state.has_escalated);
    }

    #[test]
    fn test_light_tools_stay_on_flash() {
        let mut state = RoutingState::new();

        let changed = state.process_tool_calls(&["view_claude_activity"]);
        assert!(!changed);
        assert_eq!(state.current_model, GeminiModel::Flash);

        let changed = state.process_tool_calls(&["list_instructions"]);
        assert!(!changed);
        assert_eq!(state.current_model, GeminiModel::Flash);
    }

    #[test]
    fn test_heavy_tool_triggers_escalation() {
        let mut state = RoutingState::new();

        let changed = state.process_tool_calls(&["goal"]);
        assert!(changed);
        assert_eq!(state.current_model, GeminiModel::Pro);
        assert!(state.has_escalated);
        assert!(state.escalation_reason.as_ref().unwrap().contains("goal"));
    }

    #[test]
    fn test_chain_depth_triggers_escalation() {
        let mut state = RoutingState::new();

        // First 3 calls should stay on Flash
        state.process_tool_calls(&["view_claude_activity"]);
        state.process_tool_calls(&["list_instructions"]);
        state.process_tool_calls(&["view_claude_activity"]);
        assert_eq!(state.current_model, GeminiModel::Flash);
        assert_eq!(state.tool_call_count, 3);

        // 4th call should trigger escalation
        let changed = state.process_tool_calls(&["view_claude_activity"]);
        assert!(changed);
        assert_eq!(state.current_model, GeminiModel::Pro);
        assert!(state.escalation_reason.as_ref().unwrap().contains("chain depth"));
    }

    #[test]
    fn test_no_de_escalation() {
        let mut state = RoutingState::new();

        // Escalate via heavy tool
        state.process_tool_calls(&["goal"]);
        assert_eq!(state.current_model, GeminiModel::Pro);

        // Further light tool calls shouldn't change anything
        let changed = state.process_tool_calls(&["view_claude_activity"]);
        assert!(!changed);
        assert_eq!(state.current_model, GeminiModel::Pro);
    }

    #[test]
    fn test_task_type_debugging_uses_pro() {
        let state = RoutingState::with_task_type(TaskType::Debugging);
        assert_eq!(state.current_model, GeminiModel::Pro);
        assert!(state.has_escalated);
        assert_eq!(state.thinking_level, "low");
    }

    #[test]
    fn test_task_type_planning_uses_pro() {
        let state = RoutingState::with_task_type(TaskType::Planning);
        assert_eq!(state.current_model, GeminiModel::Pro);
        assert!(state.has_escalated);
        assert_eq!(state.thinking_level, "low");
    }

    #[test]
    fn test_task_type_exploration_uses_flash() {
        let state = RoutingState::with_task_type(TaskType::Exploration);
        assert_eq!(state.current_model, GeminiModel::Flash);
        assert!(!state.has_escalated);
        assert_eq!(state.thinking_level, "minimal");
    }

    #[test]
    fn test_task_type_new_feature_uses_flash() {
        let state = RoutingState::with_task_type(TaskType::NewFeature);
        assert_eq!(state.current_model, GeminiModel::Flash);
        assert!(!state.has_escalated);
        assert_eq!(state.thinking_level, "minimal");
    }

    #[test]
    fn test_set_task_type_can_escalate() {
        let mut state = RoutingState::new();
        assert_eq!(state.current_model, GeminiModel::Flash);

        // Setting debugging task should escalate to Pro
        state.set_task_type(TaskType::Debugging);
        assert_eq!(state.current_model, GeminiModel::Pro);
        assert!(state.has_escalated);
    }

    #[test]
    fn test_set_task_type_no_deescalation() {
        // Start with Pro (debugging)
        let mut state = RoutingState::with_task_type(TaskType::Debugging);
        assert_eq!(state.current_model, GeminiModel::Pro);

        // Changing to exploration shouldn't de-escalate
        state.set_task_type(TaskType::Exploration);
        assert_eq!(state.current_model, GeminiModel::Pro); // Still Pro
        assert_eq!(state.thinking_level, "minimal"); // But thinking level updates
    }
}
