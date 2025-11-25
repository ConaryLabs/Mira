// src/patterns/types.rs
// Core types for reasoning pattern learning

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Trigger type - what activates a pattern
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TriggerType {
    /// User message contains specific keywords
    Keyword,
    /// Code context matches (e.g., working with database code)
    CodeContext,
    /// Error type matches
    ErrorType,
    /// File type/extension matches
    FileType,
    /// Operation type matches
    OperationType,
    /// User intent classification
    Intent,
    /// Combined multiple triggers
    Composite,
}

impl TriggerType {
    pub fn as_str(&self) -> &'static str {
        match self {
            TriggerType::Keyword => "keyword",
            TriggerType::CodeContext => "code_context",
            TriggerType::ErrorType => "error_type",
            TriggerType::FileType => "file_type",
            TriggerType::OperationType => "operation_type",
            TriggerType::Intent => "intent",
            TriggerType::Composite => "composite",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "keyword" => TriggerType::Keyword,
            "code_context" => TriggerType::CodeContext,
            "error_type" => TriggerType::ErrorType,
            "file_type" => TriggerType::FileType,
            "operation_type" => TriggerType::OperationType,
            "intent" => TriggerType::Intent,
            "composite" => TriggerType::Composite,
            _ => TriggerType::Keyword,
        }
    }
}

/// Step type - what kind of reasoning step
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StepType {
    /// Gather information/context
    Gather,
    /// Analyze the situation
    Analyze,
    /// Make a decision
    Decide,
    /// Generate code or content
    Generate,
    /// Validate/verify result
    Validate,
    /// Apply changes
    Apply,
    /// Explain reasoning
    Explain,
}

impl StepType {
    pub fn as_str(&self) -> &'static str {
        match self {
            StepType::Gather => "gather",
            StepType::Analyze => "analyze",
            StepType::Decide => "decide",
            StepType::Generate => "generate",
            StepType::Validate => "validate",
            StepType::Apply => "apply",
            StepType::Explain => "explain",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "gather" => StepType::Gather,
            "analyze" => StepType::Analyze,
            "decide" => StepType::Decide,
            "generate" => StepType::Generate,
            "validate" => StepType::Validate,
            "apply" => StepType::Apply,
            "explain" => StepType::Explain,
            _ => StepType::Analyze,
        }
    }
}

/// A single step in a reasoning chain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningStep {
    pub id: Option<i64>,
    pub pattern_id: String,
    pub step_number: i32,
    pub step_type: StepType,
    pub description: String,
    pub rationale: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl ReasoningStep {
    pub fn new(
        pattern_id: String,
        step_number: i32,
        step_type: StepType,
        description: String,
    ) -> Self {
        Self {
            id: None,
            pattern_id,
            step_number,
            step_type,
            description,
            rationale: None,
            created_at: Utc::now(),
        }
    }

    pub fn with_rationale(mut self, rationale: &str) -> Self {
        self.rationale = Some(rationale.to_string());
        self
    }
}

/// Applicable context for a pattern
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplicableContext {
    /// Keywords that suggest this pattern applies
    pub keywords: Vec<String>,
    /// File extensions where pattern is useful
    pub file_types: Vec<String>,
    /// Code element types (function, struct, etc.)
    pub code_elements: Vec<String>,
    /// Error codes this pattern helps with
    pub error_codes: Vec<String>,
    /// User intents this pattern addresses
    pub intents: Vec<String>,
}

impl Default for ApplicableContext {
    fn default() -> Self {
        Self {
            keywords: Vec::new(),
            file_types: Vec::new(),
            code_elements: Vec::new(),
            error_codes: Vec::new(),
            intents: Vec::new(),
        }
    }
}

/// A reasoning pattern - reusable problem-solving approach
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningPattern {
    pub id: String,
    pub project_id: Option<String>,
    pub name: String,
    pub description: String,
    pub trigger_type: TriggerType,
    /// JSON-encoded reasoning chain (high-level description)
    pub reasoning_chain: String,
    /// Template for solution generation
    pub solution_template: Option<String>,
    /// Contexts where this pattern applies
    pub applicable_contexts: ApplicableContext,
    /// Success rate (0.0 to 1.0)
    pub success_rate: f64,
    /// Total times used
    pub use_count: i32,
    /// Times used successfully
    pub success_count: i32,
    /// Estimated cost savings
    pub cost_savings_usd: f64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_used: Option<DateTime<Utc>>,
    /// Individual steps (loaded separately)
    pub steps: Vec<ReasoningStep>,
}

impl ReasoningPattern {
    pub fn new(
        name: String,
        description: String,
        trigger_type: TriggerType,
        reasoning_chain: String,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            project_id: None,
            name,
            description,
            trigger_type,
            reasoning_chain,
            solution_template: None,
            applicable_contexts: ApplicableContext::default(),
            success_rate: 1.0,
            use_count: 0,
            success_count: 0,
            cost_savings_usd: 0.0,
            created_at: now,
            updated_at: now,
            last_used: None,
            steps: Vec::new(),
        }
    }

    pub fn with_project(mut self, project_id: &str) -> Self {
        self.project_id = Some(project_id.to_string());
        self
    }

    pub fn with_template(mut self, template: &str) -> Self {
        self.solution_template = Some(template.to_string());
        self
    }

    pub fn with_contexts(mut self, contexts: ApplicableContext) -> Self {
        self.applicable_contexts = contexts;
        self
    }

    /// Add a step to the pattern
    pub fn add_step(&mut self, step_type: StepType, description: &str) {
        let step_number = self.steps.len() as i32 + 1;
        let step = ReasoningStep::new(
            self.id.clone(),
            step_number,
            step_type,
            description.to_string(),
        );
        self.steps.push(step);
    }

    /// Record successful use
    pub fn record_success(&mut self, cost_saved: f64) {
        self.use_count += 1;
        self.success_count += 1;
        self.cost_savings_usd += cost_saved;
        self.success_rate = self.success_count as f64 / self.use_count as f64;
        self.last_used = Some(Utc::now());
        self.updated_at = Utc::now();
    }

    /// Record failed use
    pub fn record_failure(&mut self) {
        self.use_count += 1;
        self.success_rate = self.success_count as f64 / self.use_count as f64;
        self.last_used = Some(Utc::now());
        self.updated_at = Utc::now();
    }

    /// Check if pattern is high-performing
    pub fn is_high_performing(&self) -> bool {
        self.success_rate >= 0.8 && self.use_count >= 5
    }

    /// Check if pattern should be deprecated
    pub fn should_deprecate(&self) -> bool {
        self.success_rate < 0.3 && self.use_count >= 10
    }
}

/// Record of a pattern being used
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternUsage {
    pub id: Option<i64>,
    pub pattern_id: String,
    pub operation_id: Option<String>,
    pub user_id: Option<String>,
    /// How well the context matched (0.0 to 1.0)
    pub context_match_score: Option<f64>,
    /// Whether it was applied successfully
    pub applied_successfully: bool,
    /// Notes about the outcome
    pub outcome_notes: Option<String>,
    /// Time saved (ms)
    pub time_saved_ms: Option<i64>,
    /// Cost saved (USD)
    pub cost_saved_usd: Option<f64>,
    pub used_at: DateTime<Utc>,
}

impl PatternUsage {
    pub fn new(pattern_id: String, applied_successfully: bool) -> Self {
        Self {
            id: None,
            pattern_id,
            operation_id: None,
            user_id: None,
            context_match_score: None,
            applied_successfully,
            outcome_notes: None,
            time_saved_ms: None,
            cost_saved_usd: None,
            used_at: Utc::now(),
        }
    }

    pub fn with_operation(mut self, operation_id: &str) -> Self {
        self.operation_id = Some(operation_id.to_string());
        self
    }

    pub fn with_user(mut self, user_id: &str) -> Self {
        self.user_id = Some(user_id.to_string());
        self
    }

    pub fn with_match_score(mut self, score: f64) -> Self {
        self.context_match_score = Some(score);
        self
    }

    pub fn with_savings(mut self, time_ms: i64, cost_usd: f64) -> Self {
        self.time_saved_ms = Some(time_ms);
        self.cost_saved_usd = Some(cost_usd);
        self
    }
}

/// Pattern match result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternMatch {
    pub pattern: ReasoningPattern,
    pub match_score: f64,
    pub match_reasons: Vec<String>,
}

/// Context for pattern matching
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MatchContext {
    /// User's message/query
    pub message: Option<String>,
    /// Current file path
    pub file_path: Option<String>,
    /// Current file content
    pub file_content: Option<String>,
    /// Error message if any
    pub error_message: Option<String>,
    /// Error code if any
    pub error_code: Option<String>,
    /// Operation type
    pub operation_type: Option<String>,
    /// Detected user intent
    pub intent: Option<String>,
    /// Keywords extracted from context
    pub keywords: Vec<String>,
}

impl MatchContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_message(mut self, message: &str) -> Self {
        self.message = Some(message.to_string());
        self
    }

    pub fn with_file(mut self, path: &str, content: Option<&str>) -> Self {
        self.file_path = Some(path.to_string());
        self.file_content = content.map(|c| c.to_string());
        self
    }

    pub fn with_error(mut self, message: &str, code: Option<&str>) -> Self {
        self.error_message = Some(message.to_string());
        self.error_code = code.map(|c| c.to_string());
        self
    }

    pub fn with_keywords(mut self, keywords: Vec<String>) -> Self {
        self.keywords = keywords;
        self
    }

    /// Extract file extension
    pub fn file_extension(&self) -> Option<String> {
        self.file_path.as_ref().and_then(|p| {
            std::path::Path::new(p)
                .extension()
                .and_then(|e| e.to_str())
                .map(|s| s.to_string())
        })
    }
}

/// Pattern statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternStats {
    pub total_patterns: i64,
    pub active_patterns: i64,
    pub deprecated_patterns: i64,
    pub total_uses: i64,
    pub successful_uses: i64,
    pub overall_success_rate: f64,
    pub total_cost_savings: f64,
    pub top_patterns: Vec<(String, i64)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pattern_creation() {
        let mut pattern = ReasoningPattern::new(
            "database_migration".to_string(),
            "Adding a new database migration".to_string(),
            TriggerType::Keyword,
            "Check migrations -> Create file -> Write SQL -> Test".to_string(),
        );

        pattern.add_step(StepType::Gather, "Check existing migrations for naming convention");
        pattern.add_step(StepType::Generate, "Create new migration file");
        pattern.add_step(StepType::Validate, "Test migration up and down");

        assert_eq!(pattern.steps.len(), 3);
        assert_eq!(pattern.steps[0].step_number, 1);
        assert_eq!(pattern.steps[2].step_number, 3);
    }

    #[test]
    fn test_success_tracking() {
        let mut pattern = ReasoningPattern::new(
            "test".to_string(),
            "Test pattern".to_string(),
            TriggerType::Keyword,
            "steps".to_string(),
        );

        pattern.record_success(0.05);
        pattern.record_success(0.03);
        pattern.record_failure();

        assert_eq!(pattern.use_count, 3);
        assert_eq!(pattern.success_count, 2);
        assert!((pattern.success_rate - 0.666).abs() < 0.01);
        assert!((pattern.cost_savings_usd - 0.08).abs() < 0.001);
    }

    #[test]
    fn test_trigger_type_conversion() {
        assert_eq!(TriggerType::from_str("keyword"), TriggerType::Keyword);
        assert_eq!(TriggerType::from_str("code_context"), TriggerType::CodeContext);
        assert_eq!(TriggerType::from_str("error_type"), TriggerType::ErrorType);
        assert_eq!(TriggerType::CodeContext.as_str(), "code_context");
    }

    #[test]
    fn test_match_context() {
        let ctx = MatchContext::new()
            .with_message("Add a new migration")
            .with_file("migrations/001_init.sql", None)
            .with_keywords(vec!["migration".to_string(), "database".to_string()]);

        assert_eq!(ctx.file_extension(), Some("sql".to_string()));
        assert!(ctx.message.unwrap().contains("migration"));
    }
}
