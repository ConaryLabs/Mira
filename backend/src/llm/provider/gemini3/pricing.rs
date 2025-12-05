// src/llm/provider/gemini3/pricing.rs
// Gemini 3 Pro pricing calculations with tier awareness

use serde::{Deserialize, Serialize};

/// Pricing tier based on context size
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PricingTier {
    /// Standard pricing: <200k context ($2/$12 per 1M tokens)
    Standard,
    /// Large context pricing: >200k context ($4/$18 per 1M tokens)
    LargeContext,
}

impl PricingTier {
    pub fn as_str(&self) -> &'static str {
        match self {
            PricingTier::Standard => "standard",
            PricingTier::LargeContext => "large_context",
        }
    }

    /// Get the pricing multiplier relative to standard tier
    pub fn cost_multiplier(&self) -> f64 {
        match self {
            PricingTier::Standard => 1.0,
            PricingTier::LargeContext => 2.0, // Input is 2x, output is 1.5x
        }
    }
}

impl std::fmt::Display for PricingTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PricingTier::Standard => write!(f, "Standard (<200k)"),
            PricingTier::LargeContext => write!(f, "Large Context (>200k)"),
        }
    }
}

/// Context size warning level
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextWarning {
    /// No warning - well under threshold
    None,
    /// Approaching threshold (>180k, 90%)
    Approaching,
    /// Very close to threshold (>190k, 95%)
    NearThreshold,
    /// Over threshold - using large context pricing
    OverThreshold,
}

impl ContextWarning {
    pub fn message(&self) -> Option<&'static str> {
        match self {
            ContextWarning::None => None,
            ContextWarning::Approaching => Some("Context approaching 200k threshold (90%)"),
            ContextWarning::NearThreshold => Some("Context very close to 200k threshold (95%)"),
            ContextWarning::OverThreshold => Some("Using large context pricing (>200k tokens)"),
        }
    }
}

/// Result of cost calculation with tier and warning info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostResult {
    /// Total cost in USD
    pub cost: f64,
    /// Which pricing tier was used
    pub tier: PricingTier,
    /// Context size warning level
    pub warning: ContextWarning,
    /// Input tokens
    pub tokens_input: i64,
    /// Output tokens
    pub tokens_output: i64,
}

/// Gemini 3 Pro Preview pricing (per 1M tokens)
/// Source: https://ai.google.dev/gemini-api/docs/pricing
/// Model: gemini-3-pro-preview (released Nov 2025)
pub struct Gemini3Pricing;

impl Gemini3Pricing {
    /// Input token price per 1M tokens (USD) - under 200k context
    pub const INPUT_PRICE_PER_M: f64 = 2.00;
    /// Input token price per 1M tokens (USD) - over 200k context
    pub const INPUT_PRICE_PER_M_LARGE: f64 = 4.00;
    /// Output token price per 1M tokens (USD) - under 200k context
    pub const OUTPUT_PRICE_PER_M: f64 = 12.00;
    /// Output token price per 1M tokens (USD) - over 200k context
    pub const OUTPUT_PRICE_PER_M_LARGE: f64 = 18.00;
    /// Large context threshold (tokens)
    pub const LARGE_CONTEXT_THRESHOLD: i64 = 200_000;
    /// Warning threshold - approaching (90% of limit)
    pub const WARNING_THRESHOLD_APPROACHING: i64 = 180_000;
    /// Warning threshold - near (95% of limit)
    pub const WARNING_THRESHOLD_NEAR: i64 = 190_000;

    /// Determine pricing tier based on input tokens
    pub fn get_tier(tokens_input: i64) -> PricingTier {
        if tokens_input > Self::LARGE_CONTEXT_THRESHOLD {
            PricingTier::LargeContext
        } else {
            PricingTier::Standard
        }
    }

    /// Get context warning level based on input tokens
    pub fn get_warning(tokens_input: i64) -> ContextWarning {
        if tokens_input > Self::LARGE_CONTEXT_THRESHOLD {
            ContextWarning::OverThreshold
        } else if tokens_input > Self::WARNING_THRESHOLD_NEAR {
            ContextWarning::NearThreshold
        } else if tokens_input > Self::WARNING_THRESHOLD_APPROACHING {
            ContextWarning::Approaching
        } else {
            ContextWarning::None
        }
    }

    /// Calculate cost from token usage (standard context)
    pub fn calculate_cost(tokens_input: i64, tokens_output: i64) -> f64 {
        let input_cost = (tokens_input as f64 / 1_000_000.0) * Self::INPUT_PRICE_PER_M;
        let output_cost = (tokens_output as f64 / 1_000_000.0) * Self::OUTPUT_PRICE_PER_M;
        input_cost + output_cost
    }

    /// Calculate cost with large context pricing (over 200k tokens)
    pub fn calculate_cost_large_context(tokens_input: i64, tokens_output: i64) -> f64 {
        let input_cost = (tokens_input as f64 / 1_000_000.0) * Self::INPUT_PRICE_PER_M_LARGE;
        let output_cost = (tokens_output as f64 / 1_000_000.0) * Self::OUTPUT_PRICE_PER_M_LARGE;
        input_cost + output_cost
    }

    /// Calculate cost with automatic tier selection
    pub fn calculate_cost_auto(tokens_input: i64, tokens_output: i64) -> f64 {
        if tokens_input > Self::LARGE_CONTEXT_THRESHOLD {
            Self::calculate_cost_large_context(tokens_input, tokens_output)
        } else {
            Self::calculate_cost(tokens_input, tokens_output)
        }
    }

    /// Calculate cost with full tier and warning information
    pub fn calculate_cost_with_info(tokens_input: i64, tokens_output: i64) -> CostResult {
        let tier = Self::get_tier(tokens_input);
        let warning = Self::get_warning(tokens_input);
        let cost = match tier {
            PricingTier::Standard => Self::calculate_cost(tokens_input, tokens_output),
            PricingTier::LargeContext => {
                Self::calculate_cost_large_context(tokens_input, tokens_output)
            }
        };

        CostResult {
            cost,
            tier,
            warning,
            tokens_input,
            tokens_output,
        }
    }

    /// Check if context should be truncated to stay in standard tier
    pub fn should_truncate_for_budget(tokens_input: i64, enforce_standard_tier: bool) -> bool {
        enforce_standard_tier && tokens_input > Self::LARGE_CONTEXT_THRESHOLD
    }

    /// Calculate how many tokens need to be trimmed to stay under threshold
    pub fn tokens_to_trim(tokens_input: i64) -> i64 {
        if tokens_input > Self::LARGE_CONTEXT_THRESHOLD {
            // Trim to 95% of threshold to leave some headroom
            tokens_input - (Self::LARGE_CONTEXT_THRESHOLD * 95 / 100)
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pricing_tier_detection() {
        assert_eq!(Gemini3Pricing::get_tier(100_000), PricingTier::Standard);
        assert_eq!(Gemini3Pricing::get_tier(200_000), PricingTier::Standard);
        assert_eq!(Gemini3Pricing::get_tier(200_001), PricingTier::LargeContext);
        assert_eq!(Gemini3Pricing::get_tier(500_000), PricingTier::LargeContext);
    }

    #[test]
    fn test_context_warnings() {
        assert_eq!(Gemini3Pricing::get_warning(100_000), ContextWarning::None);
        assert_eq!(
            Gemini3Pricing::get_warning(180_001),
            ContextWarning::Approaching
        );
        assert_eq!(
            Gemini3Pricing::get_warning(190_001),
            ContextWarning::NearThreshold
        );
        assert_eq!(
            Gemini3Pricing::get_warning(200_001),
            ContextWarning::OverThreshold
        );
    }

    #[test]
    fn test_cost_calculation() {
        // Standard tier: 100k input, 10k output
        // Input: 0.1 * $2 = $0.20
        // Output: 0.01 * $12 = $0.12
        // Total: $0.32
        let cost = Gemini3Pricing::calculate_cost(100_000, 10_000);
        assert!((cost - 0.32).abs() < 0.001);

        // Large context: 300k input, 10k output
        // Input: 0.3 * $4 = $1.20
        // Output: 0.01 * $18 = $0.18
        // Total: $1.38
        let cost = Gemini3Pricing::calculate_cost_large_context(300_000, 10_000);
        assert!((cost - 1.38).abs() < 0.001);
    }

    #[test]
    fn test_cost_with_info() {
        let result = Gemini3Pricing::calculate_cost_with_info(185_000, 5_000);
        assert_eq!(result.tier, PricingTier::Standard);
        assert_eq!(result.warning, ContextWarning::Approaching);

        let result = Gemini3Pricing::calculate_cost_with_info(250_000, 5_000);
        assert_eq!(result.tier, PricingTier::LargeContext);
        assert_eq!(result.warning, ContextWarning::OverThreshold);
    }

    #[test]
    fn test_tokens_to_trim() {
        assert_eq!(Gemini3Pricing::tokens_to_trim(100_000), 0);
        assert_eq!(Gemini3Pricing::tokens_to_trim(200_000), 0);
        // 250k - 190k (95% of 200k) = 60k to trim
        assert_eq!(Gemini3Pricing::tokens_to_trim(250_000), 60_000);
    }
}
