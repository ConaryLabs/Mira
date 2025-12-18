//! Session spend tracking
//!
//! Tracks token usage and estimated cost per session with configurable warnings.

use crate::responses::Usage;
use std::sync::atomic::{AtomicU64, Ordering};

/// GPT-5.2 pricing (per 1M tokens) - update if pricing changes
const INPUT_PRICE_PER_M: f64 = 15.0;   // $15 per 1M input tokens
const OUTPUT_PRICE_PER_M: f64 = 60.0;  // $60 per 1M output tokens
const CACHED_PRICE_PER_M: f64 = 1.875; // $1.875 per 1M cached input tokens (87.5% discount)

/// Session spend tracker
pub struct SpendTracker {
    /// Total input tokens (non-cached)
    input_tokens: AtomicU64,
    /// Total cached input tokens
    cached_tokens: AtomicU64,
    /// Total output tokens
    output_tokens: AtomicU64,
    /// Session budget in cents (0 = unlimited)
    budget_cents: u64,
    /// Warning thresholds in cents
    warn_thresholds: Vec<u64>,
    /// Which warnings have been shown
    warnings_shown: std::sync::Mutex<Vec<bool>>,
}

impl SpendTracker {
    /// Create a new spend tracker
    ///
    /// Default thresholds: warn at $1, $2, $5, $10
    pub fn new() -> Self {
        let thresholds = vec![100, 200, 500, 1000]; // cents
        let warnings_shown = vec![false; thresholds.len()];
        Self {
            input_tokens: AtomicU64::new(0),
            cached_tokens: AtomicU64::new(0),
            output_tokens: AtomicU64::new(0),
            budget_cents: 0,
            warn_thresholds: thresholds,
            warnings_shown: std::sync::Mutex::new(warnings_shown),
        }
    }

    /// Set session budget (in dollars)
    #[allow(dead_code)]
    pub fn with_budget(mut self, dollars: f64) -> Self {
        self.budget_cents = (dollars * 100.0) as u64;
        self
    }

    /// Add usage from a turn
    pub fn add_usage(&self, usage: &Usage) {
        let cached = usage.cached_tokens() as u64;
        let non_cached = (usage.input_tokens as u64).saturating_sub(cached);

        self.input_tokens.fetch_add(non_cached, Ordering::Relaxed);
        self.cached_tokens.fetch_add(cached, Ordering::Relaxed);
        self.output_tokens.fetch_add(usage.output_tokens as u64, Ordering::Relaxed);
    }

    /// Get total estimated cost in cents
    pub fn total_cents(&self) -> u64 {
        let input = self.input_tokens.load(Ordering::Relaxed);
        let cached = self.cached_tokens.load(Ordering::Relaxed);
        let output = self.output_tokens.load(Ordering::Relaxed);

        let input_cost = (input as f64 / 1_000_000.0) * INPUT_PRICE_PER_M * 100.0;
        let cached_cost = (cached as f64 / 1_000_000.0) * CACHED_PRICE_PER_M * 100.0;
        let output_cost = (output as f64 / 1_000_000.0) * OUTPUT_PRICE_PER_M * 100.0;

        (input_cost + cached_cost + output_cost) as u64
    }

    /// Get formatted spend string
    pub fn format_spend(&self) -> String {
        let cents = self.total_cents();
        if cents < 100 {
            format!("{}¢", cents)
        } else {
            format!("${:.2}", cents as f64 / 100.0)
        }
    }

    /// Check if we've exceeded budget
    pub fn over_budget(&self) -> bool {
        self.budget_cents > 0 && self.total_cents() > self.budget_cents
    }

    /// Check for warning thresholds and return any new warnings
    pub fn check_warnings(&self) -> Option<String> {
        let cents = self.total_cents();
        let mut warnings = self.warnings_shown.lock().expect("warnings_shown mutex poisoned");

        for (i, &threshold) in self.warn_thresholds.iter().enumerate() {
            if cents >= threshold && !warnings[i] {
                warnings[i] = true;
                let dollars = threshold as f64 / 100.0;
                return Some(format!(
                    "⚠️  Session spend has reached ${:.0}. Current total: {}",
                    dollars,
                    self.format_spend()
                ));
            }
        }
        None
    }

    /// Get summary stats
    pub fn summary(&self) -> String {
        let input = self.input_tokens.load(Ordering::Relaxed);
        let cached = self.cached_tokens.load(Ordering::Relaxed);
        let output = self.output_tokens.load(Ordering::Relaxed);
        let total_input = input + cached;
        let cache_pct = if total_input > 0 {
            (cached as f64 / total_input as f64) * 100.0
        } else {
            0.0
        };

        format!(
            "Session: {} spent | {}K in ({:.0}% cached) / {}K out",
            self.format_spend(),
            total_input / 1000,
            cache_pct,
            output / 1000
        )
    }
}

impl Default for SpendTracker {
    fn default() -> Self {
        Self::new()
    }
}
