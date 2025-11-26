// backend/src/context_oracle/types.rs
// Types for the Context Oracle

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Configuration for context gathering
#[derive(Debug, Clone)]
pub struct ContextConfig {
    /// Include semantic code search results
    pub include_code_search: bool,
    /// Include call graph context
    pub include_call_graph: bool,
    /// Include co-change suggestions
    pub include_cochange: bool,
    /// Include historical fixes for errors
    pub include_historical_fixes: bool,
    /// Include design pattern context
    pub include_patterns: bool,
    /// Include reasoning pattern suggestions
    pub include_reasoning_patterns: bool,
    /// Include recent build errors
    pub include_build_errors: bool,
    /// Include author expertise
    pub include_expertise: bool,
    /// Maximum tokens for context (budget-aware)
    pub max_context_tokens: usize,
    /// Maximum code search results
    pub max_code_results: usize,
    /// Maximum co-change suggestions
    pub max_cochange_suggestions: usize,
    /// Maximum historical fixes
    pub max_historical_fixes: usize,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            include_code_search: true,
            include_call_graph: true,
            include_cochange: true,
            include_historical_fixes: true,
            include_patterns: true,
            include_reasoning_patterns: true,
            include_build_errors: true,
            include_expertise: false, // Off by default to save tokens
            max_context_tokens: 8000,
            max_code_results: 10,
            max_cochange_suggestions: 5,
            max_historical_fixes: 3,
        }
    }
}

impl ContextConfig {
    /// Minimal config for simple queries
    pub fn minimal() -> Self {
        Self {
            include_code_search: true,
            include_call_graph: false,
            include_cochange: false,
            include_historical_fixes: false,
            include_patterns: false,
            include_reasoning_patterns: false,
            include_build_errors: false,
            include_expertise: false,
            max_context_tokens: 4000,
            max_code_results: 5,
            max_cochange_suggestions: 0,
            max_historical_fixes: 0,
        }
    }

    /// Full config for complex operations
    pub fn full() -> Self {
        Self {
            include_code_search: true,
            include_call_graph: true,
            include_cochange: true,
            include_historical_fixes: true,
            include_patterns: true,
            include_reasoning_patterns: true,
            include_build_errors: true,
            include_expertise: true,
            max_context_tokens: 16000,
            max_code_results: 20,
            max_cochange_suggestions: 10,
            max_historical_fixes: 5,
        }
    }

    /// Error-focused config
    pub fn for_error() -> Self {
        Self {
            include_code_search: true,
            include_call_graph: true,
            include_cochange: false,
            include_historical_fixes: true,
            include_patterns: true,
            include_reasoning_patterns: true,
            include_build_errors: true,
            include_expertise: false,
            max_context_tokens: 12000,
            max_code_results: 10,
            max_cochange_suggestions: 0,
            max_historical_fixes: 5,
        }
    }

    /// Select config based on budget usage percentage
    ///
    /// - If budget usage > 80%: minimal config (save tokens)
    /// - If budget usage 40-80%: standard config (balanced)
    /// - If budget usage < 40%: full config (maximize context)
    pub fn for_budget(daily_usage_percent: f64, monthly_usage_percent: f64) -> Self {
        // Use the more restrictive of daily or monthly
        let usage_percent = daily_usage_percent.max(monthly_usage_percent);

        if usage_percent > 80.0 {
            Self::minimal()
        } else if usage_percent > 40.0 {
            Self::default() // Standard config
        } else {
            Self::full()
        }
    }

    /// Select config for error handling based on budget
    ///
    /// Error handling gets priority, but respects budget constraints
    pub fn for_error_with_budget(daily_usage_percent: f64, monthly_usage_percent: f64) -> Self {
        let usage_percent = daily_usage_percent.max(monthly_usage_percent);

        if usage_percent > 90.0 {
            // Very tight budget: minimal error config
            Self {
                include_code_search: true,
                include_call_graph: false,
                include_cochange: false,
                include_historical_fixes: true,
                include_patterns: false,
                include_reasoning_patterns: false,
                include_build_errors: true,
                include_expertise: false,
                max_context_tokens: 4000,
                max_code_results: 3,
                max_cochange_suggestions: 0,
                max_historical_fixes: 2,
            }
        } else if usage_percent > 60.0 {
            // Moderate budget: reduced error config
            Self {
                include_code_search: true,
                include_call_graph: true,
                include_cochange: false,
                include_historical_fixes: true,
                include_patterns: false,
                include_reasoning_patterns: true,
                include_build_errors: true,
                include_expertise: false,
                max_context_tokens: 8000,
                max_code_results: 5,
                max_cochange_suggestions: 0,
                max_historical_fixes: 3,
            }
        } else {
            // Comfortable budget: full error config
            Self::for_error()
        }
    }
}

/// Budget status for context config selection
#[derive(Debug, Clone)]
pub struct BudgetStatus {
    /// Daily budget usage (0.0 to 100.0+)
    pub daily_usage_percent: f64,
    /// Monthly budget usage (0.0 to 100.0+)
    pub monthly_usage_percent: f64,
    /// Amount spent today (USD)
    pub daily_spent_usd: f64,
    /// Daily budget limit (USD)
    pub daily_limit_usd: f64,
    /// Amount spent this month (USD)
    pub monthly_spent_usd: f64,
    /// Monthly budget limit (USD)
    pub monthly_limit_usd: f64,
}

impl BudgetStatus {
    /// Create a new budget status
    pub fn new(
        daily_spent: f64,
        daily_limit: f64,
        monthly_spent: f64,
        monthly_limit: f64,
    ) -> Self {
        let daily_usage_percent = if daily_limit > 0.0 {
            (daily_spent / daily_limit) * 100.0
        } else {
            0.0
        };

        let monthly_usage_percent = if monthly_limit > 0.0 {
            (monthly_spent / monthly_limit) * 100.0
        } else {
            0.0
        };

        Self {
            daily_usage_percent,
            monthly_usage_percent,
            daily_spent_usd: daily_spent,
            daily_limit_usd: daily_limit,
            monthly_spent_usd: monthly_spent,
            monthly_limit_usd: monthly_limit,
        }
    }

    /// Get the appropriate context config based on budget status
    pub fn get_config(&self) -> ContextConfig {
        ContextConfig::for_budget(self.daily_usage_percent, self.monthly_usage_percent)
    }

    /// Get error-focused config based on budget status
    pub fn get_error_config(&self) -> ContextConfig {
        ContextConfig::for_error_with_budget(self.daily_usage_percent, self.monthly_usage_percent)
    }

    /// Check if budget is critical (>90% used)
    pub fn is_critical(&self) -> bool {
        self.daily_usage_percent > 90.0 || self.monthly_usage_percent > 90.0
    }

    /// Check if budget is low (>70% used)
    pub fn is_low(&self) -> bool {
        self.daily_usage_percent > 70.0 || self.monthly_usage_percent > 70.0
    }

    /// Get remaining daily budget (USD)
    pub fn daily_remaining(&self) -> f64 {
        (self.daily_limit_usd - self.daily_spent_usd).max(0.0)
    }

    /// Get remaining monthly budget (USD)
    pub fn monthly_remaining(&self) -> f64 {
        (self.monthly_limit_usd - self.monthly_spent_usd).max(0.0)
    }
}

/// Request for context gathering
#[derive(Debug, Clone)]
pub struct ContextRequest {
    /// User's query or message
    pub query: String,
    /// Current file being worked on (if any)
    pub current_file: Option<String>,
    /// Project ID
    pub project_id: Option<String>,
    /// Session ID for memory context
    pub session_id: String,
    /// User ID
    pub user_id: Option<String>,
    /// Error message (if context is for error resolution)
    pub error_message: Option<String>,
    /// Error code
    pub error_code: Option<String>,
    /// Configuration
    pub config: ContextConfig,
}

impl ContextRequest {
    pub fn new(query: String, session_id: String) -> Self {
        Self {
            query,
            session_id,
            current_file: None,
            project_id: None,
            user_id: None,
            error_message: None,
            error_code: None,
            config: ContextConfig::default(),
        }
    }

    pub fn with_project(mut self, project_id: &str) -> Self {
        self.project_id = Some(project_id.to_string());
        self
    }

    pub fn with_file(mut self, file_path: &str) -> Self {
        self.current_file = Some(file_path.to_string());
        self
    }

    pub fn with_error(mut self, message: &str, code: Option<&str>) -> Self {
        self.error_message = Some(message.to_string());
        self.error_code = code.map(|c| c.to_string());
        self.config = ContextConfig::for_error();
        self
    }

    pub fn with_config(mut self, config: ContextConfig) -> Self {
        self.config = config;
        self
    }
}

/// Code context from semantic search
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeContext {
    /// Relevant code elements
    pub elements: Vec<CodeElement>,
    /// Relevance score (0.0 to 1.0)
    pub relevance: f64,
}

/// A code element with context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeElement {
    pub name: String,
    pub element_type: String,
    pub file_path: String,
    pub content: String,
    pub line_number: Option<i32>,
}

/// Call graph context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallGraphContext {
    /// Functions that call the target
    pub callers: Vec<String>,
    /// Functions called by the target
    pub callees: Vec<String>,
    /// Impact analysis summary
    pub impact_summary: Option<String>,
}

/// Co-change suggestion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CochangeSuggestion {
    pub file_path: String,
    pub confidence: f64,
    pub reason: String,
    pub change_count: i32,
}

/// Historical fix information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoricalFixInfo {
    pub commit_hash: String,
    pub commit_message: String,
    pub fix_description: String,
    pub similarity: f64,
    pub files_changed: Vec<String>,
}

/// Design pattern context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternContext {
    pub pattern_type: String,
    pub pattern_name: String,
    pub description: String,
    pub relevant_files: Vec<String>,
    pub confidence: f64,
}

/// Reasoning pattern suggestion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningPatternSuggestion {
    pub pattern_id: String,
    pub pattern_name: String,
    pub description: String,
    pub match_score: f64,
    pub match_reasons: Vec<String>,
    pub suggested_steps: Vec<String>,
}

/// Build error context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildErrorContext {
    pub error_hash: String,
    pub error_message: String,
    pub file_path: Option<String>,
    pub line_number: Option<i32>,
    pub category: String,
    pub occurrence_count: i32,
    pub last_seen: DateTime<Utc>,
    pub suggested_fix: Option<String>,
}

/// Author expertise context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpertiseContext {
    pub author: String,
    pub expertise_areas: Vec<String>,
    pub overall_score: f64,
    pub relevant_files: Vec<String>,
}

/// Complete gathered context from all systems
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatheredContext {
    /// Code search results
    pub code_context: Option<CodeContext>,
    /// Call graph information
    pub call_graph: Option<CallGraphContext>,
    /// Co-change suggestions
    pub cochange_suggestions: Vec<CochangeSuggestion>,
    /// Historical fixes for similar errors
    pub historical_fixes: Vec<HistoricalFixInfo>,
    /// Detected design patterns
    pub design_patterns: Vec<PatternContext>,
    /// Reasoning pattern suggestions
    pub reasoning_patterns: Vec<ReasoningPatternSuggestion>,
    /// Recent build errors
    pub build_errors: Vec<BuildErrorContext>,
    /// Expert authors for the area
    pub expertise: Vec<ExpertiseContext>,
    /// Token count estimate
    pub estimated_tokens: usize,
    /// Gathering duration (ms)
    pub duration_ms: i64,
    /// Sources used
    pub sources_used: Vec<String>,
}

impl GatheredContext {
    pub fn empty() -> Self {
        Self {
            code_context: None,
            call_graph: None,
            cochange_suggestions: Vec::new(),
            historical_fixes: Vec::new(),
            design_patterns: Vec::new(),
            reasoning_patterns: Vec::new(),
            build_errors: Vec::new(),
            expertise: Vec::new(),
            estimated_tokens: 0,
            duration_ms: 0,
            sources_used: Vec::new(),
        }
    }

    /// Format context for prompt injection
    pub fn format_for_prompt(&self) -> String {
        let mut output = String::new();

        // Code context
        if let Some(ref code) = self.code_context {
            if !code.elements.is_empty() {
                output.push_str("## Relevant Code\n\n");
                for elem in &code.elements {
                    output.push_str(&format!(
                        "**{}** `{}` in `{}`\n",
                        elem.element_type, elem.name, elem.file_path
                    ));
                    if !elem.content.is_empty() {
                        output.push_str(&format!("```\n{}\n```\n\n", elem.content));
                    }
                }
            }
        }

        // Call graph
        if let Some(ref cg) = self.call_graph {
            if !cg.callers.is_empty() || !cg.callees.is_empty() {
                output.push_str("## Call Graph\n\n");
                if !cg.callers.is_empty() {
                    output.push_str(&format!("**Callers**: {}\n", cg.callers.join(", ")));
                }
                if !cg.callees.is_empty() {
                    output.push_str(&format!("**Callees**: {}\n", cg.callees.join(", ")));
                }
                if let Some(ref impact) = cg.impact_summary {
                    output.push_str(&format!("**Impact**: {}\n", impact));
                }
                output.push('\n');
            }
        }

        // Co-change suggestions
        if !self.cochange_suggestions.is_empty() {
            output.push_str("## Related Files (Often Changed Together)\n\n");
            for sug in &self.cochange_suggestions {
                output.push_str(&format!(
                    "- `{}` ({:.0}% confidence): {}\n",
                    sug.file_path,
                    sug.confidence * 100.0,
                    sug.reason
                ));
            }
            output.push('\n');
        }

        // Historical fixes
        if !self.historical_fixes.is_empty() {
            output.push_str("## Similar Past Fixes\n\n");
            for fix in &self.historical_fixes {
                output.push_str(&format!(
                    "- **{}** ({:.0}% similar): {}\n",
                    &fix.commit_hash[..7.min(fix.commit_hash.len())],
                    fix.similarity * 100.0,
                    fix.fix_description
                ));
            }
            output.push('\n');
        }

        // Design patterns
        if !self.design_patterns.is_empty() {
            output.push_str("## Detected Patterns\n\n");
            for pat in &self.design_patterns {
                output.push_str(&format!(
                    "- **{}** ({}): {}\n",
                    pat.pattern_name, pat.pattern_type, pat.description
                ));
            }
            output.push('\n');
        }

        // Reasoning patterns
        if !self.reasoning_patterns.is_empty() {
            output.push_str("## Suggested Approach\n\n");
            for rp in &self.reasoning_patterns {
                output.push_str(&format!(
                    "**{}** ({:.0}% match)\n",
                    rp.pattern_name,
                    rp.match_score * 100.0
                ));
                output.push_str(&format!("{}\n", rp.description));
                if !rp.suggested_steps.is_empty() {
                    output.push_str("Steps:\n");
                    for (i, step) in rp.suggested_steps.iter().enumerate() {
                        output.push_str(&format!("{}. {}\n", i + 1, step));
                    }
                }
                output.push('\n');
            }
        }

        // Build errors
        if !self.build_errors.is_empty() {
            output.push_str("## Recent Build Errors\n\n");
            for err in &self.build_errors {
                output.push_str(&format!("- **{}**: {}\n", err.category, err.error_message));
                if let Some(ref fix) = err.suggested_fix {
                    output.push_str(&format!("  Suggested fix: {}\n", fix));
                }
            }
            output.push('\n');
        }

        // Expertise
        if !self.expertise.is_empty() {
            output.push_str("## Domain Experts\n\n");
            for exp in &self.expertise {
                output.push_str(&format!(
                    "- **{}** ({:.0}% expertise): {}\n",
                    exp.author,
                    exp.overall_score * 100.0,
                    exp.expertise_areas.join(", ")
                ));
            }
            output.push('\n');
        }

        output
    }

    /// Check if context is essentially empty
    pub fn is_empty(&self) -> bool {
        self.code_context.is_none()
            && self.call_graph.is_none()
            && self.cochange_suggestions.is_empty()
            && self.historical_fixes.is_empty()
            && self.design_patterns.is_empty()
            && self.reasoning_patterns.is_empty()
            && self.build_errors.is_empty()
            && self.expertise.is_empty()
    }
}

/// Statistics about context gathering
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatheringStats {
    pub total_queries: i64,
    pub avg_duration_ms: f64,
    pub avg_tokens: f64,
    pub cache_hit_rate: f64,
    pub sources_used: Vec<(String, i64)>,
}
