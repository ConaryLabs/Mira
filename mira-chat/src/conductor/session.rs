//! Conductor Session - High-level integration for REPL
//!
//! Provides a simple interface for running tasks through the conductor
//! from the REPL or other entry points.

use std::sync::Arc;

use super::{
    ast_context::{ContextBuilder, FileOutline},
    config::ConductorConfig,
    executor::{StepExecutor, StepResult, ToolHandler},
    observability::SessionMetrics,
    orchestrator::{Conductor, ConductorError, TaskResult},
    planning::{parse_plan, Plan, PlanStep},
};
use crate::context::MiraContext;
use crate::provider::{DeepSeekProvider, OpenAiProvider, Provider};

/// A conductor session for executing tasks
pub struct ConductorSession {
    /// The main conductor
    conductor: Conductor,

    /// Step executor for individual steps
    executor: StepExecutor,

    /// Session metrics
    metrics: SessionMetrics,

    /// Configuration
    config: ConductorConfig,

    /// System prompt for the session
    system_prompt: String,
}

/// Options for creating a conductor session
#[derive(Debug, Clone)]
pub struct SessionOptions {
    /// DeepSeek API key
    pub deepseek_key: String,

    /// OpenAI API key (for escalation)
    pub openai_key: Option<String>,

    /// Configuration profile
    pub config: ConductorConfig,

    /// System prompt (if None, uses default + context injection)
    pub system_prompt: Option<String>,

    /// Project path for context
    pub project_path: Option<String>,

    /// Mira context for injection (corrections, guidelines)
    pub mira_context: Option<MiraContext>,
}

impl ConductorSession {
    /// Create a new conductor session
    pub fn new(options: SessionOptions) -> Self {
        // Create providers
        let reasoner: Arc<dyn Provider> = Arc::new(
            DeepSeekProvider::new_reasoner(options.deepseek_key.clone())
        );
        let chat: Arc<dyn Provider> = Arc::new(
            DeepSeekProvider::new_chat(options.deepseek_key.clone())
        );
        let gpt: Option<Arc<dyn Provider>> = options.openai_key.map(|key| {
            Arc::new(OpenAiProvider::new(key)) as Arc<dyn Provider>
        });

        // Create conductor
        let conductor = Conductor::new(
            reasoner,
            Arc::clone(&chat),
            gpt,
            options.config.clone(),
        );

        // Create executor
        let executor = StepExecutor::new(chat, options.config.clone());

        // Build system prompt: explicit > context-injected > default
        let system_prompt = options.system_prompt.unwrap_or_else(|| {
            match &options.mira_context {
                Some(ctx) => build_conductor_prompt(ctx),
                None => default_system_prompt(),
            }
        });

        Self {
            conductor,
            executor,
            metrics: SessionMetrics::start(),
            config: options.config,
            system_prompt,
        }
    }

    /// Execute a task through the conductor
    pub async fn execute(&mut self, task: &str, context: &str) -> Result<SessionResult, ConductorError> {
        let result = self.conductor.execute_task(
            task,
            &self.system_prompt,
            context,
        ).await?;

        // Update metrics from result
        for _ in 0..result.plan.steps.len() {
            // Would need actual step results here
        }

        if result.escalated {
            self.metrics.set_escalated("Task complexity");
        }

        self.metrics.finish();

        Ok(SessionResult {
            output: result.output,
            plan: result.plan,
            metrics: self.metrics.clone(),
            escalated: result.escalated,
        })
    }

    /// Execute a task with planning only (no execution)
    pub async fn plan_only(&mut self, task: &str, context: &str) -> Result<Plan, ConductorError> {
        // Get plan from reasoner without executing
        let plan_prompt = format!(
            r#"Analyze this task and create a structured execution plan.

TASK: {}

CONTEXT:
{}

Create a JSON plan with this structure:
{{
    "summary": "Brief description",
    "steps": [
        {{
            "index": 0,
            "step_type": "read|edit|create|delete|command|search|verify",
            "description": "What this step does",
            "context_files": [],
            "expected_tools": [],
            "depends_on": []
        }}
    ],
    "affected_files": [],
    "complexity": "low|medium|high",
    "chat_executable": true,
    "verification": []
}}

Output ONLY the JSON plan."#,
            task, context
        );

        // For now, return empty plan - full implementation would call reasoner
        Ok(Plan::empty())
    }

    /// Get current metrics
    pub fn metrics(&self) -> &SessionMetrics {
        &self.metrics
    }

    /// Get configuration
    pub fn config(&self) -> &ConductorConfig {
        &self.config
    }

    /// Build context for a set of files
    pub fn build_context(&self, files: &[(&str, &str)], max_tokens: usize) -> String {
        let mut builder = ContextBuilder::new(max_tokens);

        for (path, content) in files {
            let outline = FileOutline::extract(path, content);
            if !builder.add_outline(&outline) {
                break; // Budget exhausted
            }
        }

        builder.build()
    }
}

/// Result of a conductor session
#[derive(Debug)]
pub struct SessionResult {
    /// The output from execution
    pub output: String,

    /// The plan that was executed
    pub plan: Plan,

    /// Session metrics
    pub metrics: SessionMetrics,

    /// Whether we escalated to GPT-5.2
    pub escalated: bool,
}

impl SessionResult {
    /// Format a summary report
    pub fn report(&self) -> String {
        let mut lines = Vec::new();

        lines.push(format!("Plan: {} steps", self.plan.steps.len()));
        if self.escalated {
            lines.push("⚠️  Escalated to GPT-5.2".into());
        }
        lines.push(String::new());
        lines.push(self.metrics.report());

        lines.join("\n")
    }
}

/// Quick helper to create a balanced session
pub fn quick_session(deepseek_key: &str, openai_key: Option<&str>) -> ConductorSession {
    ConductorSession::new(SessionOptions {
        deepseek_key: deepseek_key.to_string(),
        openai_key: openai_key.map(|s| s.to_string()),
        config: ConductorConfig::balanced(),
        system_prompt: None,
        project_path: None,
        mira_context: None,
    })
}

/// Create a session with Mira context injection
pub fn session_with_context(
    deepseek_key: &str,
    openai_key: Option<&str>,
    context: MiraContext,
) -> ConductorSession {
    ConductorSession::new(SessionOptions {
        deepseek_key: deepseek_key.to_string(),
        openai_key: openai_key.map(|s| s.to_string()),
        config: ConductorConfig::balanced(),
        system_prompt: None,
        project_path: context.project_path.clone(),
        mira_context: Some(context),
    })
}

/// Default system prompt for conductor sessions
pub fn default_system_prompt() -> String {
    r#"You are a skilled software engineer helping to implement code changes.

Guidelines:
- Use diff format for file edits (old_string/new_string)
- Be precise and minimal in changes
- Verify changes work before marking complete
- If unsure, ask for clarification

Available tools:
- Read: Read file contents
- Edit: Edit file with old_string/new_string
- Write: Create new file
- Bash: Run shell commands
- Glob: Find files by pattern
- Grep: Search file contents"#.into()
}

/// Build conductor-specific prompt with Mira context injection
///
/// Strategy: Include ONLY corrections (code quality impact)
/// Skip: persona (DeepSeek produces code, not conversation),
///       goals (task-level, not relevant per-step),
///       memories (too noisy for focused execution)
pub fn build_conductor_prompt(ctx: &MiraContext) -> String {
    let mut sections = Vec::new();

    // Base role - code-focused, no personality
    sections.push(
        "You are a skilled software engineer executing code changes.".to_string()
    );

    // Corrections - CRITICAL: these directly improve output quality
    if !ctx.corrections.is_empty() {
        let mut lines = vec!["\n## Code Quality Rules (follow strictly)".to_string()];
        for c in &ctx.corrections {
            // Format for fast parsing by the model
            lines.push(format!("- ❌ {} → ✓ {}", c.what_was_wrong, c.what_is_right));
        }
        sections.push(lines.join("\n"));
    }

    // Minimal tool guidance
    sections.push(r#"
## Execution Rules
- Use diff format for edits (old_string/new_string)
- Be precise and minimal in changes
- Output ONLY code when asked for implementation
- Use .expect("reason") not .unwrap()"#.to_string());

    sections.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::Correction;

    #[test]
    fn test_session_options() {
        let options = SessionOptions {
            deepseek_key: "test-key".into(),
            openai_key: Some("openai-key".into()),
            config: ConductorConfig::default(),
            system_prompt: Some("Test prompt".into()),
            project_path: Some("/test".into()),
            mira_context: None,
        };

        assert_eq!(options.deepseek_key, "test-key");
        assert!(options.openai_key.is_some());
    }

    #[test]
    fn test_context_building() {
        let code = "fn main() {\n    println!(\"Hello\");\n}";
        let outline = FileOutline::extract("test.rs", code);

        let mut builder = ContextBuilder::new(1000);
        assert!(builder.add_outline(&outline));

        let context = builder.build();
        assert!(context.contains("test.rs"));
    }

    #[test]
    fn test_default_system_prompt() {
        let prompt = default_system_prompt();
        assert!(prompt.contains("diff format"));
        assert!(prompt.contains("Edit"));
    }

    #[test]
    fn test_build_conductor_prompt_empty() {
        let ctx = MiraContext::default();
        let prompt = build_conductor_prompt(&ctx);

        assert!(prompt.contains("software engineer"));
        assert!(prompt.contains("Execution Rules"));
        assert!(!prompt.contains("Code Quality Rules")); // No corrections
    }

    #[test]
    fn test_build_conductor_prompt_with_corrections() {
        let ctx = MiraContext {
            corrections: vec![
                Correction {
                    what_was_wrong: "Using .unwrap()".into(),
                    what_is_right: "Use .expect(\"reason\")".into(),
                    correction_type: "style".into(),
                    rationale: None,
                },
            ],
            ..Default::default()
        };
        let prompt = build_conductor_prompt(&ctx);

        assert!(prompt.contains("Code Quality Rules"));
        assert!(prompt.contains("Using .unwrap()"));
        assert!(prompt.contains(".expect(\"reason\")"));
        // Should NOT include persona-related content
        assert!(!prompt.contains("Mira"));
        assert!(!prompt.contains("Active Goals"));
    }
}
