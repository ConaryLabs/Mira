// src/llm/provider/gemini3/pricing.rs
// Gemini 3 Pro pricing calculations

/// Gemini 3 Pro Preview pricing (per 1M tokens)
/// Source: https://ai.google.dev/gemini-api/docs/pricing
/// Model: gemini-3-pro-preview (released Nov 2025)
pub struct Gemini3Pricing;

impl Gemini3Pricing {
    /// Input token price per 1M tokens (USD) - under 200k context
    const INPUT_PRICE_PER_M: f64 = 2.00;
    /// Input token price per 1M tokens (USD) - over 200k context
    const INPUT_PRICE_PER_M_LARGE: f64 = 4.00;
    /// Output token price per 1M tokens (USD) - under 200k context
    const OUTPUT_PRICE_PER_M: f64 = 12.00;
    /// Output token price per 1M tokens (USD) - over 200k context
    const OUTPUT_PRICE_PER_M_LARGE: f64 = 18.00;
    /// Large context threshold
    const LARGE_CONTEXT_THRESHOLD: i64 = 200_000;

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
}
