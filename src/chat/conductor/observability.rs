//! Observability - Logging, cost tracking, and metrics
//!
//! Provides structured observability for the conductor:
//! - Detailed cost breakdowns (DeepSeek vs GPT-5.2 savings)
//! - Token usage tracking per model
//! - Latency metrics
//! - Session summaries

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Complete session metrics
#[derive(Debug, Clone, Default)]
pub struct SessionMetrics {
    /// When the session started
    pub started: Option<Instant>,

    /// When the session ended
    pub ended: Option<Instant>,

    /// Cost breakdown
    pub cost: CostBreakdown,

    /// Token usage by model
    pub tokens: TokenUsage,

    /// Latency metrics
    pub latency: LatencyMetrics,

    /// Step statistics
    pub steps: StepMetrics,

    /// Whether we escalated to GPT-5.2
    pub escalated: bool,

    /// Escalation reason if any
    pub escalation_reason: Option<String>,
}

/// Detailed cost breakdown
#[derive(Debug, Clone, Default)]
pub struct CostBreakdown {
    /// DeepSeek Reasoner cost
    pub deepseek_reasoner: ModelCost,

    /// DeepSeek Chat cost
    pub deepseek_chat: ModelCost,

    /// GPT-5.2 cost (if escalated)
    pub gpt_5_2: ModelCost,
}

/// Cost for a single model
#[derive(Debug, Clone, Default)]
pub struct ModelCost {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: u64,
    pub cached_tokens: u64,
    pub requests: u32,
}

impl ModelCost {
    /// Calculate cost for DeepSeek models
    pub fn deepseek_cost(&self) -> f64 {
        // DeepSeek pricing (per million tokens)
        const INPUT: f64 = 0.27;
        const OUTPUT: f64 = 0.41;
        // Reasoning tokens same as output for Reasoner
        const REASONING: f64 = 0.41;
        // Cached input is discounted
        const CACHED_INPUT: f64 = 0.07;

        let regular_input = self.input_tokens.saturating_sub(self.cached_tokens);
        (regular_input as f64 / 1_000_000.0) * INPUT
            + (self.cached_tokens as f64 / 1_000_000.0) * CACHED_INPUT
            + (self.output_tokens as f64 / 1_000_000.0) * OUTPUT
            + (self.reasoning_tokens as f64 / 1_000_000.0) * REASONING
    }

    /// Calculate cost for GPT-5.2
    pub fn gpt_cost(&self) -> f64 {
        // GPT-5.2 pricing (per million tokens)
        const INPUT: f64 = 2.50;
        const OUTPUT: f64 = 10.00;
        const REASONING: f64 = 10.00;
        const CACHED_INPUT: f64 = 0.625; // 75% discount

        let regular_input = self.input_tokens.saturating_sub(self.cached_tokens);
        (regular_input as f64 / 1_000_000.0) * INPUT
            + (self.cached_tokens as f64 / 1_000_000.0) * CACHED_INPUT
            + (self.output_tokens as f64 / 1_000_000.0) * OUTPUT
            + (self.reasoning_tokens as f64 / 1_000_000.0) * REASONING
    }
}

impl CostBreakdown {
    /// Total actual cost
    pub fn total_cost(&self) -> f64 {
        self.deepseek_reasoner.deepseek_cost()
            + self.deepseek_chat.deepseek_cost()
            + self.gpt_5_2.gpt_cost()
    }

    /// Equivalent GPT-5.2 cost (what it would have cost using only GPT)
    pub fn equivalent_gpt_cost(&self) -> f64 {
        // All tokens priced at GPT-5.2 rates
        let total_input = self.deepseek_reasoner.input_tokens
            + self.deepseek_chat.input_tokens
            + self.gpt_5_2.input_tokens;
        let total_output = self.deepseek_reasoner.output_tokens
            + self.deepseek_chat.output_tokens
            + self.gpt_5_2.output_tokens;
        let total_reasoning = self.deepseek_reasoner.reasoning_tokens
            + self.deepseek_chat.reasoning_tokens
            + self.gpt_5_2.reasoning_tokens;
        let total_cached = self.deepseek_reasoner.cached_tokens
            + self.deepseek_chat.cached_tokens
            + self.gpt_5_2.cached_tokens;

        let regular_input = total_input.saturating_sub(total_cached);

        const INPUT: f64 = 2.50;
        const OUTPUT: f64 = 10.00;
        const REASONING: f64 = 10.00;
        const CACHED: f64 = 0.625;

        (regular_input as f64 / 1_000_000.0) * INPUT
            + (total_cached as f64 / 1_000_000.0) * CACHED
            + (total_output as f64 / 1_000_000.0) * OUTPUT
            + (total_reasoning as f64 / 1_000_000.0) * REASONING
    }

    /// Cost savings percentage
    pub fn savings_percentage(&self) -> f64 {
        let actual = self.total_cost();
        let equivalent = self.equivalent_gpt_cost();
        if equivalent > 0.0 {
            (1.0 - (actual / equivalent)) * 100.0
        } else {
            0.0
        }
    }

    /// Format cost summary as string
    pub fn summary(&self) -> String {
        let actual = self.total_cost();
        let equivalent = self.equivalent_gpt_cost();
        let savings = self.savings_percentage();

        format!(
            "Cost: ${:.4} (saved ${:.4}, {:.1}% vs GPT-5.2)",
            actual,
            equivalent - actual,
            savings
        )
    }

    /// Detailed breakdown as multi-line string
    pub fn detailed(&self) -> String {
        let mut lines = Vec::new();

        if self.deepseek_reasoner.requests > 0 {
            lines.push(format!(
                "  Reasoner: {} reqs, {}in/{}out/{}reasoning → ${:.4}",
                self.deepseek_reasoner.requests,
                format_tokens(self.deepseek_reasoner.input_tokens),
                format_tokens(self.deepseek_reasoner.output_tokens),
                format_tokens(self.deepseek_reasoner.reasoning_tokens),
                self.deepseek_reasoner.deepseek_cost()
            ));
        }

        if self.deepseek_chat.requests > 0 {
            lines.push(format!(
                "  Chat: {} reqs, {}in/{}out → ${:.4}",
                self.deepseek_chat.requests,
                format_tokens(self.deepseek_chat.input_tokens),
                format_tokens(self.deepseek_chat.output_tokens),
                self.deepseek_chat.deepseek_cost()
            ));
        }

        if self.gpt_5_2.requests > 0 {
            lines.push(format!(
                "  GPT-5.2: {} reqs, {}in/{}out → ${:.4}",
                self.gpt_5_2.requests,
                format_tokens(self.gpt_5_2.input_tokens),
                format_tokens(self.gpt_5_2.output_tokens),
                self.gpt_5_2.gpt_cost()
            ));
        }

        lines.push(self.summary());
        lines.join("\n")
    }
}

/// Token usage across all models
#[derive(Debug, Clone, Default)]
pub struct TokenUsage {
    pub total_input: u64,
    pub total_output: u64,
    pub total_reasoning: u64,
    pub total_cached: u64,
    pub by_model: HashMap<String, (u64, u64)>, // model -> (input, output)
}

impl TokenUsage {
    /// Add usage for a model
    pub fn add(&mut self, model: &str, input: u64, output: u64, reasoning: u64, cached: u64) {
        self.total_input += input;
        self.total_output += output;
        self.total_reasoning += reasoning;
        self.total_cached += cached;

        let entry = self.by_model.entry(model.to_string()).or_insert((0, 0));
        entry.0 += input;
        entry.1 += output;
    }

    /// Cache hit rate
    pub fn cache_rate(&self) -> f64 {
        if self.total_input > 0 {
            (self.total_cached as f64 / self.total_input as f64) * 100.0
        } else {
            0.0
        }
    }

    pub fn summary(&self) -> String {
        format!(
            "Tokens: {}in/{}out ({}% cached)",
            format_tokens(self.total_input),
            format_tokens(self.total_output),
            self.cache_rate() as u32
        )
    }
}

/// Latency metrics
#[derive(Debug, Clone, Default)]
pub struct LatencyMetrics {
    /// Planning phase duration
    pub planning: Option<Duration>,

    /// Per-step durations
    pub steps: Vec<Duration>,

    /// Total execution time
    pub total: Option<Duration>,

    /// Time to first token (streaming)
    pub time_to_first_token: Option<Duration>,
}

impl LatencyMetrics {
    pub fn average_step_time(&self) -> Option<Duration> {
        if self.steps.is_empty() {
            None
        } else {
            let sum: Duration = self.steps.iter().sum();
            Some(sum / self.steps.len() as u32)
        }
    }

    pub fn summary(&self) -> String {
        let mut parts = Vec::new();

        if let Some(planning) = self.planning {
            parts.push(format!("plan:{:.1}s", planning.as_secs_f64()));
        }

        if let Some(avg) = self.average_step_time() {
            parts.push(format!("step:{:.1}s avg", avg.as_secs_f64()));
        }

        if let Some(total) = self.total {
            parts.push(format!("total:{:.1}s", total.as_secs_f64()));
        }

        if parts.is_empty() {
            "Latency: (no data)".into()
        } else {
            format!("Latency: {}", parts.join(", "))
        }
    }
}

/// Step execution metrics
#[derive(Debug, Clone, Default)]
pub struct StepMetrics {
    pub total: usize,
    pub completed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub tool_calls: usize,
    pub tool_repairs: usize,
    pub tool_failures: usize,
}

impl StepMetrics {
    pub fn success_rate(&self) -> f64 {
        let executed = self.completed + self.failed;
        if executed > 0 {
            (self.completed as f64 / executed as f64) * 100.0
        } else {
            0.0
        }
    }

    pub fn tool_success_rate(&self) -> f64 {
        if self.tool_calls > 0 {
            ((self.tool_calls - self.tool_failures) as f64 / self.tool_calls as f64) * 100.0
        } else {
            100.0
        }
    }

    pub fn summary(&self) -> String {
        format!(
            "Steps: {}/{} completed ({:.0}%), {} tools ({} repaired, {} failed)",
            self.completed,
            self.total,
            self.success_rate(),
            self.tool_calls,
            self.tool_repairs,
            self.tool_failures
        )
    }
}

impl SessionMetrics {
    /// Create new metrics, starting the timer
    pub fn start() -> Self {
        Self {
            started: Some(Instant::now()),
            ..Default::default()
        }
    }

    /// Mark session as complete
    pub fn finish(&mut self) {
        self.ended = Some(Instant::now());
        if let (Some(start), Some(end)) = (self.started, self.ended) {
            self.latency.total = Some(end.duration_since(start));
        }
    }

    /// Record planning phase completion
    pub fn planning_complete(&mut self, duration: Duration) {
        self.latency.planning = Some(duration);
    }

    /// Record step completion
    pub fn step_complete(&mut self, duration: Duration, success: bool, tool_calls: usize, repairs: usize, failures: usize) {
        self.latency.steps.push(duration);
        self.steps.total += 1;
        if success {
            self.steps.completed += 1;
        } else {
            self.steps.failed += 1;
        }
        self.steps.tool_calls += tool_calls;
        self.steps.tool_repairs += repairs;
        self.steps.tool_failures += failures;
    }

    /// Record DeepSeek Reasoner usage
    pub fn add_reasoner_usage(&mut self, input: u64, output: u64, reasoning: u64, cached: u64) {
        self.cost.deepseek_reasoner.input_tokens += input;
        self.cost.deepseek_reasoner.output_tokens += output;
        self.cost.deepseek_reasoner.reasoning_tokens += reasoning;
        self.cost.deepseek_reasoner.cached_tokens += cached;
        self.cost.deepseek_reasoner.requests += 1;
        self.tokens.add("deepseek-reasoner", input, output, reasoning, cached);
    }

    /// Record DeepSeek Chat usage
    pub fn add_chat_usage(&mut self, input: u64, output: u64, cached: u64) {
        self.cost.deepseek_chat.input_tokens += input;
        self.cost.deepseek_chat.output_tokens += output;
        self.cost.deepseek_chat.cached_tokens += cached;
        self.cost.deepseek_chat.requests += 1;
        self.tokens.add("deepseek-chat", input, output, 0, cached);
    }

    /// Record GPT-5.2 usage (escalation)
    pub fn add_gpt_usage(&mut self, input: u64, output: u64, reasoning: u64, cached: u64) {
        self.cost.gpt_5_2.input_tokens += input;
        self.cost.gpt_5_2.output_tokens += output;
        self.cost.gpt_5_2.reasoning_tokens += reasoning;
        self.cost.gpt_5_2.cached_tokens += cached;
        self.cost.gpt_5_2.requests += 1;
        self.tokens.add("gpt-5.2", input, output, reasoning, cached);
    }

    /// Mark as escalated
    pub fn set_escalated(&mut self, reason: &str) {
        self.escalated = true;
        self.escalation_reason = Some(reason.into());
    }

    /// Full summary report
    pub fn report(&self) -> String {
        let mut lines = Vec::new();

        lines.push("═══ Session Summary ═══".into());

        if self.escalated {
            lines.push(format!(
                "⚠️  Escalated to GPT-5.2: {}",
                self.escalation_reason.as_deref().unwrap_or("unknown")
            ));
        }

        lines.push(self.steps.summary());
        lines.push(self.tokens.summary());
        lines.push(self.latency.summary());
        lines.push(String::new());
        lines.push("Cost breakdown:".into());
        lines.push(self.cost.detailed());

        lines.join("\n")
    }
}

/// Format token count for display (e.g., "1.2K", "3.4M")
fn format_tokens(count: u64) -> String {
    if count >= 1_000_000 {
        format!("{:.1}M", count as f64 / 1_000_000.0)
    } else if count >= 1_000 {
        format!("{:.1}K", count as f64 / 1_000.0)
    } else {
        count.to_string()
    }
}

/// Log a conductor event with structured data
#[macro_export]
macro_rules! conductor_event {
    ($event:expr, $($field:tt)*) => {
        tracing::info!(
            target: "conductor",
            event = $event,
            $($field)*
        )
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_cost_deepseek() {
        let cost = ModelCost {
            input_tokens: 1_000_000,
            output_tokens: 100_000,
            reasoning_tokens: 50_000,
            cached_tokens: 200_000,
            requests: 5,
        };

        // (800K * 0.27 + 200K * 0.07 + 100K * 0.41 + 50K * 0.41) / 1M
        // = 0.216 + 0.014 + 0.041 + 0.0205 = ~0.2915
        let actual = cost.deepseek_cost();
        assert!((actual - 0.2915).abs() < 0.001);
    }

    #[test]
    fn test_cost_savings() {
        let mut breakdown = CostBreakdown::default();
        breakdown.deepseek_chat.input_tokens = 100_000;
        breakdown.deepseek_chat.output_tokens = 10_000;
        breakdown.deepseek_chat.requests = 1;

        let actual = breakdown.total_cost();
        let equivalent = breakdown.equivalent_gpt_cost();
        let savings = breakdown.savings_percentage();

        // DeepSeek should be much cheaper
        assert!(actual < equivalent);
        assert!(savings > 80.0); // At least 80% savings
    }

    #[test]
    fn test_format_tokens() {
        assert_eq!(format_tokens(500), "500");
        assert_eq!(format_tokens(1500), "1.5K");
        assert_eq!(format_tokens(1_500_000), "1.5M");
    }

    #[test]
    fn test_session_metrics() {
        let mut metrics = SessionMetrics::start();

        metrics.add_reasoner_usage(10000, 5000, 2000, 1000);
        metrics.add_chat_usage(8000, 3000, 500);
        metrics.step_complete(Duration::from_secs(2), true, 3, 1, 0);
        metrics.step_complete(Duration::from_secs(1), true, 2, 0, 0);
        metrics.finish();

        assert_eq!(metrics.steps.completed, 2);
        assert_eq!(metrics.steps.tool_calls, 5);
        assert!(metrics.latency.total.is_some());

        let report = metrics.report();
        assert!(report.contains("Steps:"));
        assert!(report.contains("Cost"));
    }
}
