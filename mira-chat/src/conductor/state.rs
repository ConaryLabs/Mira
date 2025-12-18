//! Conductor state machine
//!
//! Manages transitions between planning and execution phases.

use std::time::{Duration, Instant};

/// Current state of the conductor
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConductorState {
    /// Initial state - ready to accept a task
    Idle,

    /// Gathering context and forming understanding
    Understanding {
        started: Instant,
    },

    /// Reasoner is creating/refining the plan
    Planning {
        started: Instant,
        attempt: u32,
    },

    /// Chat is executing a plan step
    Executing {
        step_index: usize,
        total_steps: usize,
        started: Instant,
    },

    /// Waiting for tool results
    WaitingForTools {
        step_index: usize,
        pending_calls: usize,
    },

    /// Verifying results after execution
    Verifying {
        started: Instant,
    },

    /// Task completed successfully
    Completed {
        duration: Duration,
    },

    /// Escalating to GPT-5.2 due to failure
    Escalating {
        reason: EscalationReason,
    },

    /// Task failed after all retries
    Failed {
        reason: String,
    },
}

/// Why we're escalating to GPT-5.2
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EscalationReason {
    /// Reasoner failed to produce valid plan after N attempts
    PlanningFailed { attempts: u32 },

    /// Chat tool calls failed repeatedly
    ToolCallsFailed { attempts: u32 },

    /// Context budget exceeded (can't fit required context)
    ContextBudgetExceeded { required: u32, budget: u32 },

    /// Verification failed after execution
    VerificationFailed { reason: String },

    /// User explicitly requested GPT-5.2
    UserRequested,

    /// Task complexity exceeds DeepSeek capabilities
    TaskTooComplex { reason: String },
}

/// A transition between states
#[derive(Debug, Clone)]
pub struct StateTransition {
    pub from: ConductorState,
    pub to: ConductorState,
    pub reason: String,
    pub timestamp: Instant,
}

/// A single execution step tracked by the conductor
#[derive(Debug, Clone)]
pub struct ExecutionStep {
    pub index: usize,
    pub description: String,
    pub started: Option<Instant>,
    pub completed: Option<Instant>,
    pub status: StepStatus,
    pub tool_calls: Vec<TrackedToolCall>,
    pub output: Option<String>,
    pub error: Option<String>,
}

/// Status of a step
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
    Skipped,
}

/// A tool call tracked during execution
#[derive(Debug, Clone)]
pub struct TrackedToolCall {
    pub call_id: String,
    pub name: String,
    pub arguments: String,
    pub started: Instant,
    pub completed: Option<Instant>,
    pub result: Option<String>,
    pub success: bool,
}

impl ConductorState {
    /// Check if this state allows accepting new tasks
    pub fn can_accept_task(&self) -> bool {
        matches!(self, Self::Idle | Self::Completed { .. } | Self::Failed { .. })
    }

    /// Check if this is a terminal state
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed { .. } | Self::Failed { .. })
    }

    /// Check if actively processing
    pub fn is_active(&self) -> bool {
        matches!(
            self,
            Self::Understanding { .. }
                | Self::Planning { .. }
                | Self::Executing { .. }
                | Self::WaitingForTools { .. }
                | Self::Verifying { .. }
        )
    }

    /// Get human-readable status
    pub fn status_message(&self) -> String {
        match self {
            Self::Idle => "Ready".into(),
            Self::Understanding { .. } => "Understanding the task...".into(),
            Self::Planning { attempt, .. } => {
                if *attempt > 1 {
                    format!("Planning (attempt {})...", attempt)
                } else {
                    "Planning...".into()
                }
            }
            Self::Executing {
                step_index,
                total_steps,
                ..
            } => {
                format!("Executing step {}/{}", step_index + 1, total_steps)
            }
            Self::WaitingForTools { pending_calls, .. } => {
                format!("Waiting for {} tool result(s)...", pending_calls)
            }
            Self::Verifying { .. } => "Verifying results...".into(),
            Self::Completed { duration } => {
                format!("Completed in {:.1}s", duration.as_secs_f64())
            }
            Self::Escalating { reason } => {
                format!("Escalating to GPT-5.2: {}", reason.short_description())
            }
            Self::Failed { reason } => format!("Failed: {}", reason),
        }
    }
}

impl EscalationReason {
    pub fn short_description(&self) -> &str {
        match self {
            Self::PlanningFailed { .. } => "planning failed",
            Self::ToolCallsFailed { .. } => "tool calls failed",
            Self::ContextBudgetExceeded { .. } => "context too large",
            Self::VerificationFailed { .. } => "verification failed",
            Self::UserRequested => "user requested",
            Self::TaskTooComplex { .. } => "task too complex",
        }
    }
}

impl ExecutionStep {
    pub fn new(index: usize, description: String) -> Self {
        Self {
            index,
            description,
            started: None,
            completed: None,
            status: StepStatus::Pending,
            tool_calls: Vec::new(),
            output: None,
            error: None,
        }
    }

    pub fn start(&mut self) {
        self.started = Some(Instant::now());
        self.status = StepStatus::InProgress;
    }

    pub fn complete(&mut self, output: Option<String>) {
        self.completed = Some(Instant::now());
        self.status = StepStatus::Completed;
        self.output = output;
    }

    pub fn fail(&mut self, error: String) {
        self.completed = Some(Instant::now());
        self.status = StepStatus::Failed;
        self.error = Some(error);
    }

    pub fn duration(&self) -> Option<Duration> {
        match (self.started, self.completed) {
            (Some(s), Some(c)) => Some(c.duration_since(s)),
            (Some(s), None) => Some(Instant::now().duration_since(s)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_transitions() {
        let idle = ConductorState::Idle;
        assert!(idle.can_accept_task());
        assert!(!idle.is_terminal());
        assert!(!idle.is_active());

        let planning = ConductorState::Planning {
            started: Instant::now(),
            attempt: 1,
        };
        assert!(!planning.can_accept_task());
        assert!(!planning.is_terminal());
        assert!(planning.is_active());

        let completed = ConductorState::Completed {
            duration: Duration::from_secs(5),
        };
        assert!(completed.can_accept_task());
        assert!(completed.is_terminal());
        assert!(!completed.is_active());
    }

    #[test]
    fn test_execution_step() {
        let mut step = ExecutionStep::new(0, "Test step".into());
        assert_eq!(step.status, StepStatus::Pending);
        assert!(step.duration().is_none());

        step.start();
        assert_eq!(step.status, StepStatus::InProgress);
        assert!(step.duration().is_some());

        step.complete(Some("Result".into()));
        assert_eq!(step.status, StepStatus::Completed);
        assert!(step.output.is_some());
    }
}
