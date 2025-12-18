//! Conductor orchestrator - the main state machine
//!
//! Coordinates between Reasoner (planning) and Chat (execution).

use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;

use super::config::ConductorConfig;
use super::mira_intel::MiraIntel;
use super::planning::{parse_plan, Plan, PlanStep};
use super::state::{
    ConductorState, EscalationReason, ExecutionStep, StateTransition, StepStatus,
};
use crate::provider::{
    ChatRequest, ChatResponse, Provider, StreamEvent, ToolContinueRequest, ToolResult,
};

/// Cost tracking for a conductor session
#[derive(Debug, Clone, Default)]
pub struct CostTracker {
    pub deepseek_input_tokens: u64,
    pub deepseek_output_tokens: u64,
    pub gpt_input_tokens: u64,
    pub gpt_output_tokens: u64,
}

impl CostTracker {
    /// Calculate total cost (approximate)
    pub fn total_cost(&self) -> f64 {
        // DeepSeek pricing (per million)
        const DS_INPUT: f64 = 0.27;
        const DS_OUTPUT: f64 = 0.41;
        // GPT-5.2 pricing (per million)
        const GPT_INPUT: f64 = 2.50;
        const GPT_OUTPUT: f64 = 10.00;

        let ds_cost = (self.deepseek_input_tokens as f64 / 1_000_000.0) * DS_INPUT
            + (self.deepseek_output_tokens as f64 / 1_000_000.0) * DS_OUTPUT;
        let gpt_cost = (self.gpt_input_tokens as f64 / 1_000_000.0) * GPT_INPUT
            + (self.gpt_output_tokens as f64 / 1_000_000.0) * GPT_OUTPUT;

        ds_cost + gpt_cost
    }

    /// Calculate equivalent GPT-5.2 cost (for savings calculation)
    pub fn equivalent_gpt_cost(&self) -> f64 {
        const GPT_INPUT: f64 = 2.50;
        const GPT_OUTPUT: f64 = 10.00;

        let total_input = self.deepseek_input_tokens + self.gpt_input_tokens;
        let total_output = self.deepseek_output_tokens + self.gpt_output_tokens;

        (total_input as f64 / 1_000_000.0) * GPT_INPUT
            + (total_output as f64 / 1_000_000.0) * GPT_OUTPUT
    }

    /// Calculate savings percentage
    pub fn savings_percentage(&self) -> f64 {
        let actual = self.total_cost();
        let equivalent = self.equivalent_gpt_cost();
        if equivalent > 0.0 {
            1.0 - (actual / equivalent)
        } else {
            0.0
        }
    }
}

/// The Conductor - orchestrates between Reasoner and Chat
pub struct Conductor {
    /// Configuration
    config: ConductorConfig,

    /// DeepSeek Reasoner provider (planning)
    reasoner: Arc<dyn Provider>,

    /// DeepSeek Chat provider (execution)
    chat: Arc<dyn Provider>,

    /// GPT-5.2 provider (escalation fallback)
    gpt: Option<Arc<dyn Provider>>,

    /// Mira intelligence (error fixes, rejected approaches, cochange)
    mira: Option<Arc<MiraIntel>>,

    /// Current state
    state: RwLock<ConductorState>,

    /// State transition history
    transitions: RwLock<Vec<StateTransition>>,

    /// Current plan (if any)
    plan: RwLock<Option<Plan>>,

    /// Execution steps tracking
    steps: RwLock<Vec<ExecutionStep>>,

    /// Cost tracking
    cost: RwLock<CostTracker>,

    /// Task start time
    task_started: RwLock<Option<Instant>>,
}

impl Conductor {
    /// Create a new Conductor
    pub fn new(
        reasoner: Arc<dyn Provider>,
        chat: Arc<dyn Provider>,
        gpt: Option<Arc<dyn Provider>>,
        config: ConductorConfig,
    ) -> Self {
        Self {
            config,
            reasoner,
            chat,
            gpt,
            mira: None,
            state: RwLock::new(ConductorState::Idle),
            transitions: RwLock::new(Vec::new()),
            plan: RwLock::new(None),
            steps: RwLock::new(Vec::new()),
            cost: RwLock::new(CostTracker::default()),
            task_started: RwLock::new(None),
        }
    }

    /// Add Mira intelligence layer
    pub fn with_mira(mut self, mira: Arc<MiraIntel>) -> Self {
        self.mira = Some(mira);
        self
    }

    /// Get current state
    pub async fn state(&self) -> ConductorState {
        self.state.read().await.clone()
    }

    /// Get current plan
    pub async fn plan(&self) -> Option<Plan> {
        self.plan.read().await.clone()
    }

    /// Get cost tracker
    pub async fn cost(&self) -> CostTracker {
        self.cost.read().await.clone()
    }

    /// Transition to a new state
    async fn transition(&self, new_state: ConductorState, reason: &str) {
        let mut state = self.state.write().await;
        let transition = StateTransition {
            from: state.clone(),
            to: new_state.clone(),
            reason: reason.into(),
            timestamp: Instant::now(),
        };
        *state = new_state;

        let mut transitions = self.transitions.write().await;
        transitions.push(transition);
    }

    /// Execute a complete task
    pub async fn execute_task(
        &self,
        task: &str,
        system_prompt: &str,
        context: &str,
    ) -> Result<TaskResult, ConductorError> {
        // Check if we can accept a new task
        {
            let state = self.state.read().await;
            if !state.can_accept_task() {
                return Err(ConductorError::Busy(state.status_message()));
            }
        }

        // Check for auto-escalation keywords
        if self.config.should_escalate_task(task) && self.gpt.is_some() {
            self.transition(
                ConductorState::Escalating {
                    reason: EscalationReason::TaskTooComplex {
                        reason: "Task contains escalation keywords".into(),
                    },
                },
                "Auto-escalating based on task keywords",
            )
            .await;
            return self.execute_with_gpt(task, system_prompt, context).await;
        }

        // Start the task
        *self.task_started.write().await = Some(Instant::now());
        self.transition(
            ConductorState::Understanding {
                started: Instant::now(),
            },
            "Starting task",
        )
        .await;

        // Phase 1: Planning with Reasoner
        let plan = match self.create_plan(task, system_prompt, context).await {
            Ok(plan) => plan,
            Err(e) => {
                if self.config.auto_escalate && self.gpt.is_some() {
                    self.transition(
                        ConductorState::Escalating {
                            reason: EscalationReason::PlanningFailed { attempts: 3 },
                        },
                        &format!("Planning failed: {}", e),
                    )
                    .await;
                    return self.execute_with_gpt(task, system_prompt, context).await;
                } else {
                    self.transition(
                        ConductorState::Failed {
                            reason: format!("Planning failed: {}", e),
                        },
                        "Planning failed, no escalation",
                    )
                    .await;
                    return Err(ConductorError::PlanningFailed(e.to_string()));
                }
            }
        };

        // Store the plan
        *self.plan.write().await = Some(plan.clone());

        // Initialize execution steps
        {
            let mut steps = self.steps.write().await;
            *steps = plan
                .steps
                .iter()
                .map(|s| ExecutionStep::new(s.index, s.description.clone()))
                .collect();
        }

        // Phase 2: Execute plan with Chat
        let mut completed_steps: Vec<usize> = Vec::new();
        let mut output = String::new();

        for step in &plan.steps {
            self.transition(
                ConductorState::Executing {
                    step_index: step.index,
                    total_steps: plan.steps.len(),
                    started: Instant::now(),
                },
                &format!("Executing step {}: {}", step.index + 1, step.description),
            )
            .await;

            match self.execute_step(step, system_prompt, &output).await {
                Ok(step_output) => {
                    completed_steps.push(step.index);

                    // Progress checkpoint: structured summary for next step
                    let checkpoint = format!(
                        "\n--- Step {} Complete ---\nDone: {}\nRemaining: {} of {} steps\n",
                        step.index + 1,
                        &step.description,
                        plan.steps.len() - completed_steps.len(),
                        plan.steps.len()
                    );
                    output.push_str(&checkpoint);
                    output.push_str(&step_output);
                    output.push('\n');

                    // Mark step complete
                    let mut steps = self.steps.write().await;
                    if let Some(exec_step) = steps.get_mut(step.index) {
                        exec_step.complete(Some(step_output));
                    }
                }
                Err(e) => {
                    // Look up similar error fixes from Mira (if available)
                    let fix_hints = if let Some(ref mira) = self.mira {
                        let fixes = mira.find_similar_fixes(&e.to_string()).await;
                        MiraIntel::format_fix_hints(&fixes)
                    } else {
                        String::new()
                    };

                    // If we have fix hints, try once more with the hints injected
                    if !fix_hints.is_empty() {
                        tracing::info!("Found similar error fixes, retrying with hints");
                        let retry_context = format!("{}\n{}", output, fix_hints);
                        if let Ok(step_output) = self.execute_step(step, system_prompt, &retry_context).await {
                            completed_steps.push(step.index);
                            output.push_str(&step_output);
                            output.push('\n');

                            let mut steps = self.steps.write().await;
                            if let Some(exec_step) = steps.get_mut(step.index) {
                                exec_step.complete(Some(step_output));
                            }
                            continue; // Success after retry with fix hints
                        }
                    }

                    // Mark step failed (retry didn't help or no hints available)
                    {
                        let mut steps = self.steps.write().await;
                        if let Some(exec_step) = steps.get_mut(step.index) {
                            exec_step.fail(e.to_string());
                        }
                    }

                    if self.config.auto_escalate && self.gpt.is_some() {
                        self.transition(
                            ConductorState::Escalating {
                                reason: EscalationReason::ToolCallsFailed { attempts: 2 },
                            },
                            &format!("Step {} failed: {}", step.index, e),
                        )
                        .await;
                        return self.execute_with_gpt(task, system_prompt, context).await;
                    } else {
                        self.transition(
                            ConductorState::Failed {
                                reason: format!("Step {} failed: {}", step.index, e),
                            },
                            "Execution failed, no escalation",
                        )
                        .await;
                        return Err(ConductorError::ExecutionFailed(e.to_string()));
                    }
                }
            }
        }

        // Calculate duration
        let duration = self
            .task_started
            .read()
            .await
            .map(|s| s.elapsed())
            .unwrap_or_default();

        self.transition(
            ConductorState::Completed { duration },
            "Task completed successfully",
        )
        .await;

        let cost = self.cost.read().await.clone();
        Ok(TaskResult {
            output,
            plan,
            cost,
            duration,
            escalated: false,
        })
    }

    /// Create a plan using the Reasoner
    async fn create_plan(
        &self,
        task: &str,
        system_prompt: &str,
        context: &str,
    ) -> Result<Plan, ConductorError> {
        self.transition(
            ConductorState::Planning {
                started: Instant::now(),
                attempt: 1,
            },
            "Starting planning phase",
        )
        .await;

        // Get rejected approaches from Mira (if available)
        let rejected_section = if let Some(ref mira) = self.mira {
            let rejected = mira.get_rejected_approaches(task).await;
            MiraIntel::format_rejected_approaches(&rejected)
        } else {
            String::new()
        };

        // Get cochange patterns for files mentioned in context (if available)
        // Only add if we detect file paths in the context/task
        let cochange_section = if let Some(ref mira) = self.mira {
            // Simple heuristic: look for paths in context
            let paths: Vec<&str> = context.split_whitespace()
                .chain(task.split_whitespace())
                .filter(|w| w.contains('/') && (w.ends_with(".rs") || w.ends_with(".py") || w.ends_with(".ts") || w.ends_with(".js")))
                .take(3) // Limit to first 3 files
                .collect();

            let mut all_patterns = Vec::new();
            for path in paths {
                let patterns = mira.get_cochange_patterns(path).await;
                all_patterns.extend(patterns.into_iter().take(2)); // Top 2 per file
            }
            all_patterns.truncate(5); // Cap total at 5

            MiraIntel::format_cochange_context(&all_patterns)
        } else {
            String::new()
        };

        let planning_prompt = format!(
            r#"You are a coding assistant planning tool. Analyze the task and create a structured execution plan.

TASK: {}

CONTEXT:
{}{}{}

Create a JSON plan with this structure:
{{
    "summary": "Brief description of what will be done",
    "steps": [
        {{
            "index": 0,
            "step_type": "read|edit|create|delete|command|search|verify",
            "description": "What this step does",
            "context_files": ["files to read for context"],
            "expected_tools": ["Read", "Edit", "Bash"],
            "depends_on": [],
            "diff": "unified diff format if edit",
            "target_file": "path if edit/create/delete",
            "command": "shell command if command type"
        }}
    ],
    "affected_files": ["list of files that will be modified"],
    "complexity": "low|medium|high",
    "chat_executable": true,
    "verification": ["how to verify the changes work"]
}}

RULES:
- Use EDIT steps with diffs for file modifications (never output full files)
- Keep diffs minimal and focused
- Order steps by dependency
- Include verification at the end
- Be specific about expected tool calls

Output ONLY the JSON plan, no other text."#,
            task, context, rejected_section, cochange_section
        );

        let request = ChatRequest::new("deepseek-reasoner", system_prompt, &planning_prompt);

        for attempt in 1..=self.config.max_planning_attempts {
            self.transition(
                ConductorState::Planning {
                    started: Instant::now(),
                    attempt,
                },
                &format!("Planning attempt {}", attempt),
            )
            .await;

            match self.reasoner.create(request.clone()).await {
                Ok(response) => {
                    // Track cost
                    if let Some(usage) = &response.usage {
                        let mut cost = self.cost.write().await;
                        cost.deepseek_input_tokens += usage.input_tokens as u64;
                        cost.deepseek_output_tokens += usage.output_tokens as u64;
                    }

                    // Parse the plan
                    match parse_plan(&response.text) {
                        Ok(plan) => {
                            if plan.steps.is_empty() {
                                continue; // Empty plan, retry
                            }
                            return Ok(plan);
                        }
                        Err(e) => {
                            tracing::warn!("Failed to parse plan (attempt {}): {}", attempt, e);
                            continue;
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Reasoner error (attempt {}): {}", attempt, e);
                    continue;
                }
            }
        }

        Err(ConductorError::PlanningFailed(
            "Max planning attempts exceeded".into(),
        ))
    }

    /// Execute a single plan step using Chat
    async fn execute_step(
        &self,
        step: &PlanStep,
        system_prompt: &str,
        previous_output: &str,
    ) -> Result<String, ConductorError> {
        let step_prompt = format!(
            r#"Execute this step:

STEP {}: {}

Previous output:
{}

{}{}

Execute this step using the appropriate tools. Be concise."#,
            step.index + 1,
            step.description,
            if previous_output.is_empty() {
                "(none)"
            } else {
                previous_output
            },
            step.diff
                .as_ref()
                .map(|d| format!("Apply this diff:\n```diff\n{}\n```\n", d))
                .unwrap_or_default(),
            step.command
                .as_ref()
                .map(|c| format!("Run command: {}\n", c))
                .unwrap_or_default(),
        );

        let request = ChatRequest::new("deepseek-chat", system_prompt, &step_prompt);

        match self.chat.create(request).await {
            Ok(response) => {
                // Track cost
                if let Some(usage) = &response.usage {
                    let mut cost = self.cost.write().await;
                    cost.deepseek_input_tokens += usage.input_tokens as u64;
                    cost.deepseek_output_tokens += usage.output_tokens as u64;
                }

                // TODO: Handle tool calls in a loop
                if !response.tool_calls.is_empty() {
                    // For now, just return the text - tool execution will be Phase 7
                    return Ok(format!(
                        "{}\n[Tool calls: {}]",
                        response.text,
                        response
                            .tool_calls
                            .iter()
                            .map(|t| t.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }

                Ok(response.text)
            }
            Err(e) => Err(ConductorError::ExecutionFailed(e.to_string())),
        }
    }

    /// Execute the full task with GPT-5.2 (escalation path)
    async fn execute_with_gpt(
        &self,
        task: &str,
        system_prompt: &str,
        context: &str,
    ) -> Result<TaskResult, ConductorError> {
        let gpt = self.gpt.as_ref().ok_or_else(|| {
            ConductorError::EscalationFailed("No GPT provider configured".into())
        })?;

        let prompt = format!("{}\n\nContext:\n{}", task, context);
        let request = ChatRequest::new("gpt-5.2", system_prompt, &prompt)
            .with_reasoning("medium");

        match gpt.create(request).await {
            Ok(response) => {
                // Track cost
                if let Some(usage) = &response.usage {
                    let mut cost = self.cost.write().await;
                    cost.gpt_input_tokens += usage.input_tokens as u64;
                    cost.gpt_output_tokens += usage.output_tokens as u64;
                }

                let duration = self
                    .task_started
                    .read()
                    .await
                    .map(|s| s.elapsed())
                    .unwrap_or_default();

                self.transition(
                    ConductorState::Completed { duration },
                    "Completed via GPT-5.2 escalation",
                )
                .await;

                let cost = self.cost.read().await.clone();
                Ok(TaskResult {
                    output: response.text,
                    plan: Plan::empty(),
                    cost,
                    duration,
                    escalated: true,
                })
            }
            Err(e) => {
                self.transition(
                    ConductorState::Failed {
                        reason: format!("GPT escalation failed: {}", e),
                    },
                    "Escalation failed",
                )
                .await;
                Err(ConductorError::EscalationFailed(e.to_string()))
            }
        }
    }
}

/// Result of a conductor task execution
#[derive(Debug)]
pub struct TaskResult {
    /// The output/response
    pub output: String,

    /// The plan that was executed
    pub plan: Plan,

    /// Cost tracking
    pub cost: CostTracker,

    /// Total duration
    pub duration: std::time::Duration,

    /// Whether we escalated to GPT-5.2
    pub escalated: bool,
}

/// Conductor errors
#[derive(Debug, thiserror::Error)]
pub enum ConductorError {
    #[error("Conductor is busy: {0}")]
    Busy(String),

    #[error("Planning failed: {0}")]
    PlanningFailed(String),

    #[error("Execution failed: {0}")]
    ExecutionFailed(String),

    #[error("Escalation failed: {0}")]
    EscalationFailed(String),

    #[error("Context budget exceeded: required {required} > budget {budget}")]
    ContextBudgetExceeded { required: u32, budget: u32 },

    #[error("Timeout: {0}")]
    Timeout(String),
}
