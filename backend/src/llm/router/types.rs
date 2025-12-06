// src/llm/router/types.rs
// Types for model routing system

use serde::{Deserialize, Serialize};

/// Model tier for routing decisions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ModelTier {
    /// Fast tier: GPT-5.1 Mini
    /// Use for: file listing, grep, simple queries, summaries
    /// Cost: $0.25/$2 per 1M tokens
    Fast,

    /// Voice tier: GPT-5.1 (medium reasoning effort)
    /// Use for: user-facing chat, explanations, Mira's personality
    /// Cost: $1.25/$10 per 1M tokens
    Voice,

    /// Code tier: GPT-5.1-Codex-Max (high reasoning effort)
    /// Use for: code generation, refactoring, complex code tasks
    /// Cost: $1.25/$10 per 1M tokens
    Code,

    /// Agentic tier: GPT-5.1-Codex-Max (xhigh reasoning effort)
    /// Use for: long-running autonomous tasks, migrations, large refactors
    /// Cost: $1.25/$10 per 1M tokens (can run for 24+ hours)
    Agentic,
}

impl ModelTier {
    pub fn as_str(&self) -> &'static str {
        match self {
            ModelTier::Fast => "fast",
            ModelTier::Voice => "voice",
            ModelTier::Code => "code",
            ModelTier::Agentic => "agentic",
        }
    }

    /// Get display name for logging
    pub fn display_name(&self) -> &'static str {
        match self {
            ModelTier::Fast => "Fast (GPT-5.1 Mini)",
            ModelTier::Voice => "Voice (GPT-5.1)",
            ModelTier::Code => "Code (GPT-5.1-Codex-Max)",
            ModelTier::Agentic => "Agentic (GPT-5.1-Codex-Max XHigh)",
        }
    }

    /// Approximate cost multiplier relative to Fast tier
    pub fn cost_multiplier(&self) -> f64 {
        match self {
            ModelTier::Fast => 1.0,
            ModelTier::Voice => 5.0,   // ~5x more expensive than Fast
            ModelTier::Code => 5.0,    // Same pricing as Voice (Codex-Max is $1.25/$10)
            ModelTier::Agentic => 7.0, // More output due to xhigh reasoning
        }
    }
}

impl Default for ModelTier {
    fn default() -> Self {
        ModelTier::Voice // Default to voice tier for user interactions
    }
}

impl std::fmt::Display for ModelTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Task information for routing decisions
#[derive(Debug, Clone)]
pub struct RoutingTask {
    /// Tool name being called (if any)
    pub tool_name: Option<String>,

    /// Operation kind (e.g., "code_gen", "refactor", "chat")
    pub operation_kind: Option<String>,

    /// Estimated input tokens
    pub estimated_tokens: i64,

    /// Number of files involved
    pub file_count: usize,

    /// Whether this is user-facing (chat) vs background (tool)
    pub is_user_facing: bool,

    /// Explicit tier override (if user requested specific tier)
    pub tier_override: Option<ModelTier>,

    /// Whether this is a long-running task (routes to Agentic tier)
    pub is_long_running: bool,
}

impl RoutingTask {
    /// Create a new task for routing
    pub fn new() -> Self {
        Self {
            tool_name: None,
            operation_kind: None,
            estimated_tokens: 0,
            file_count: 0,
            is_user_facing: true,
            tier_override: None,
            is_long_running: false,
        }
    }

    /// Create task from tool call
    pub fn from_tool(tool_name: &str) -> Self {
        Self {
            tool_name: Some(tool_name.to_string()),
            operation_kind: None,
            estimated_tokens: 0,
            file_count: 0,
            is_user_facing: false, // Tool calls are typically background
            tier_override: None,
            is_long_running: false,
        }
    }

    /// Create task for user chat
    pub fn user_chat() -> Self {
        Self {
            tool_name: None,
            operation_kind: Some("chat".to_string()),
            estimated_tokens: 0,
            file_count: 0,
            is_user_facing: true,
            tier_override: None,
            is_long_running: false,
        }
    }

    /// Set operation kind (marks as non-user-facing)
    pub fn with_operation(mut self, kind: &str) -> Self {
        self.operation_kind = Some(kind.to_string());
        self.is_user_facing = false; // Operations aren't simple user chat
        self
    }

    /// Set estimated tokens
    pub fn with_tokens(mut self, tokens: i64) -> Self {
        self.estimated_tokens = tokens;
        self
    }

    /// Set file count (marks as non-user-facing if count > 0)
    pub fn with_files(mut self, count: usize) -> Self {
        self.file_count = count;
        if count > 0 {
            self.is_user_facing = false; // File operations aren't simple user chat
        }
        self
    }

    /// Force a specific tier
    pub fn with_tier(mut self, tier: ModelTier) -> Self {
        self.tier_override = Some(tier);
        self
    }

    /// Mark as long-running task (routes to Agentic tier)
    pub fn with_long_running(mut self, long_running: bool) -> Self {
        self.is_long_running = long_running;
        self
    }
}

impl Default for RoutingTask {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics for routing decisions
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RoutingStats {
    /// Requests routed to Fast tier
    pub fast_requests: u64,
    /// Requests routed to Voice tier
    pub voice_requests: u64,
    /// Requests routed to Code tier
    pub code_requests: u64,
    /// Requests routed to Agentic tier
    pub agentic_requests: u64,
    /// Estimated cost savings (vs all Agentic)
    pub estimated_savings_usd: f64,
}

impl RoutingStats {
    pub fn record(&mut self, tier: ModelTier, tokens: i64) {
        match tier {
            ModelTier::Fast => self.fast_requests += 1,
            ModelTier::Voice => self.voice_requests += 1,
            ModelTier::Code => self.code_requests += 1,
            ModelTier::Agentic => self.agentic_requests += 1,
        }

        // Estimate savings: (agentic_cost - actual_cost) for each request
        // Simplified: assume 10k tokens per request
        let tokens = if tokens > 0 { tokens } else { 10_000 };
        let agentic_cost = (tokens as f64 / 1_000_000.0) * 4.0; // ~$4/M for Agentic (with reasoning)
        let actual_cost = match tier {
            ModelTier::Fast => (tokens as f64 / 1_000_000.0) * 0.25,
            ModelTier::Voice => (tokens as f64 / 1_000_000.0) * 1.25,
            ModelTier::Code => (tokens as f64 / 1_000_000.0) * 1.25,
            ModelTier::Agentic => agentic_cost,
        };
        self.estimated_savings_usd += agentic_cost - actual_cost;
    }

    pub fn total_requests(&self) -> u64 {
        self.fast_requests + self.voice_requests + self.code_requests + self.agentic_requests
    }

    pub fn fast_percentage(&self) -> f64 {
        let total = self.total_requests();
        if total == 0 {
            0.0
        } else {
            (self.fast_requests as f64 / total as f64) * 100.0
        }
    }

    /// Get percentage routed to Voice tier
    pub fn voice_percentage(&self) -> f64 {
        let total = self.total_requests();
        if total == 0 { 0.0 } else { (self.voice_requests as f64 / total as f64) * 100.0 }
    }

    /// Get percentage routed to Code tier
    pub fn code_percentage(&self) -> f64 {
        let total = self.total_requests();
        if total == 0 { 0.0 } else { (self.code_requests as f64 / total as f64) * 100.0 }
    }

    /// Get percentage routed to Agentic tier
    pub fn agentic_percentage(&self) -> f64 {
        let total = self.total_requests();
        if total == 0 { 0.0 } else { (self.agentic_requests as f64 / total as f64) * 100.0 }
    }

    /// Calculate total estimated cost for all requests
    pub fn total_cost_usd(&self) -> f64 {
        // Using average 10k tokens per request pricing
        let fast_cost = (self.fast_requests as f64) * 0.0025;      // $0.25/M * 10k
        let voice_cost = (self.voice_requests as f64) * 0.0125;    // $1.25/M * 10k
        let code_cost = (self.code_requests as f64) * 0.0125;      // $1.25/M * 10k
        let agentic_cost = (self.agentic_requests as f64) * 0.04;  // $4.00/M * 10k
        fast_cost + voice_cost + code_cost + agentic_cost
    }

    /// Calculate what cost would have been if all requests used Agentic tier
    pub fn baseline_cost_usd(&self) -> f64 {
        (self.total_requests() as f64) * 0.04  // All at Agentic pricing
    }

    /// Calculate savings percentage vs all-Agentic baseline
    pub fn savings_percentage(&self) -> f64 {
        let baseline = self.baseline_cost_usd();
        if baseline == 0.0 { 0.0 } else { (self.estimated_savings_usd / baseline) * 100.0 }
    }
}
