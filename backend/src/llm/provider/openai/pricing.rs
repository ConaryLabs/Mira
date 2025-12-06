// src/llm/provider/openai/pricing.rs
// GPT-5.1 pricing calculations

use super::types::OpenAIModel;
use serde::{Deserialize, Serialize};

/// Result of cost calculation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostResult {
    /// Total cost in USD
    pub cost: f64,
    /// Model used
    pub model: OpenAIModel,
    /// Input tokens
    pub tokens_input: i64,
    /// Output tokens
    pub tokens_output: i64,
    /// Cached tokens (if any)
    pub tokens_cached: i64,
}

/// GPT-5.1 pricing calculator
/// Source: https://openai.com/api/pricing/
/// Prices as of December 2025
pub struct OpenAIPricing;

impl OpenAIPricing {
    // GPT-5.1 pricing (per 1M tokens)
    pub const GPT51_INPUT_PRICE_PER_M: f64 = 1.25;
    pub const GPT51_OUTPUT_PRICE_PER_M: f64 = 10.00;
    pub const GPT51_CACHED_INPUT_PRICE_PER_M: f64 = 0.125; // 90% discount for cached

    // GPT-5.1 Mini pricing (per 1M tokens)
    pub const GPT51_MINI_INPUT_PRICE_PER_M: f64 = 0.25;
    pub const GPT51_MINI_OUTPUT_PRICE_PER_M: f64 = 2.00;
    pub const GPT51_MINI_CACHED_INPUT_PRICE_PER_M: f64 = 0.025; // 90% discount for cached

    // GPT-5.1-Codex-Max pricing (per 1M tokens)
    // Same as GPT-5.1 base pricing, but optimized for code workloads
    pub const GPT51_CODEX_MAX_INPUT_PRICE_PER_M: f64 = 1.25;
    pub const GPT51_CODEX_MAX_OUTPUT_PRICE_PER_M: f64 = 10.00;
    pub const GPT51_CODEX_MAX_CACHED_INPUT_PRICE_PER_M: f64 = 0.125; // 90% discount for cached

    /// Calculate cost for GPT-5.1
    pub fn calculate_cost_gpt51(tokens_input: i64, tokens_output: i64) -> f64 {
        let input_cost = (tokens_input as f64 / 1_000_000.0) * Self::GPT51_INPUT_PRICE_PER_M;
        let output_cost = (tokens_output as f64 / 1_000_000.0) * Self::GPT51_OUTPUT_PRICE_PER_M;
        input_cost + output_cost
    }

    /// Calculate cost for GPT-5.1 with cached tokens
    pub fn calculate_cost_gpt51_with_cache(
        tokens_input: i64,
        tokens_output: i64,
        tokens_cached: i64,
    ) -> f64 {
        let uncached_input = tokens_input - tokens_cached;
        let input_cost =
            (uncached_input as f64 / 1_000_000.0) * Self::GPT51_INPUT_PRICE_PER_M;
        let cached_cost =
            (tokens_cached as f64 / 1_000_000.0) * Self::GPT51_CACHED_INPUT_PRICE_PER_M;
        let output_cost = (tokens_output as f64 / 1_000_000.0) * Self::GPT51_OUTPUT_PRICE_PER_M;
        input_cost + cached_cost + output_cost
    }

    /// Calculate cost for GPT-5.1 Mini
    pub fn calculate_cost_gpt51_mini(tokens_input: i64, tokens_output: i64) -> f64 {
        let input_cost = (tokens_input as f64 / 1_000_000.0) * Self::GPT51_MINI_INPUT_PRICE_PER_M;
        let output_cost = (tokens_output as f64 / 1_000_000.0) * Self::GPT51_MINI_OUTPUT_PRICE_PER_M;
        input_cost + output_cost
    }

    /// Calculate cost for GPT-5.1 Mini with cached tokens
    pub fn calculate_cost_gpt51_mini_with_cache(
        tokens_input: i64,
        tokens_output: i64,
        tokens_cached: i64,
    ) -> f64 {
        let uncached_input = tokens_input - tokens_cached;
        let input_cost =
            (uncached_input as f64 / 1_000_000.0) * Self::GPT51_MINI_INPUT_PRICE_PER_M;
        let cached_cost =
            (tokens_cached as f64 / 1_000_000.0) * Self::GPT51_MINI_CACHED_INPUT_PRICE_PER_M;
        let output_cost =
            (tokens_output as f64 / 1_000_000.0) * Self::GPT51_MINI_OUTPUT_PRICE_PER_M;
        input_cost + cached_cost + output_cost
    }

    /// Calculate cost for GPT-5.1-Codex-Max
    pub fn calculate_cost_codex_max(tokens_input: i64, tokens_output: i64) -> f64 {
        let input_cost =
            (tokens_input as f64 / 1_000_000.0) * Self::GPT51_CODEX_MAX_INPUT_PRICE_PER_M;
        let output_cost =
            (tokens_output as f64 / 1_000_000.0) * Self::GPT51_CODEX_MAX_OUTPUT_PRICE_PER_M;
        input_cost + output_cost
    }

    /// Calculate cost for GPT-5.1-Codex-Max with cached tokens
    pub fn calculate_cost_codex_max_with_cache(
        tokens_input: i64,
        tokens_output: i64,
        tokens_cached: i64,
    ) -> f64 {
        let uncached_input = tokens_input - tokens_cached;
        let input_cost =
            (uncached_input as f64 / 1_000_000.0) * Self::GPT51_CODEX_MAX_INPUT_PRICE_PER_M;
        let cached_cost =
            (tokens_cached as f64 / 1_000_000.0) * Self::GPT51_CODEX_MAX_CACHED_INPUT_PRICE_PER_M;
        let output_cost =
            (tokens_output as f64 / 1_000_000.0) * Self::GPT51_CODEX_MAX_OUTPUT_PRICE_PER_M;
        input_cost + cached_cost + output_cost
    }

    /// Calculate cost for any model
    pub fn calculate_cost(
        model: OpenAIModel,
        tokens_input: i64,
        tokens_output: i64,
    ) -> f64 {
        match model {
            OpenAIModel::Gpt51 => Self::calculate_cost_gpt51(tokens_input, tokens_output),
            OpenAIModel::Gpt51Mini => Self::calculate_cost_gpt51_mini(tokens_input, tokens_output),
            OpenAIModel::Gpt51CodexMax => {
                Self::calculate_cost_codex_max(tokens_input, tokens_output)
            }
        }
    }

    /// Calculate cost with full details
    pub fn calculate_cost_with_info(
        model: OpenAIModel,
        tokens_input: i64,
        tokens_output: i64,
        tokens_cached: i64,
    ) -> CostResult {
        let cost = match model {
            OpenAIModel::Gpt51 => {
                Self::calculate_cost_gpt51_with_cache(tokens_input, tokens_output, tokens_cached)
            }
            OpenAIModel::Gpt51Mini => {
                Self::calculate_cost_gpt51_mini_with_cache(tokens_input, tokens_output, tokens_cached)
            }
            OpenAIModel::Gpt51CodexMax => {
                Self::calculate_cost_codex_max_with_cache(tokens_input, tokens_output, tokens_cached)
            }
        };

        CostResult {
            cost,
            model,
            tokens_input,
            tokens_output,
            tokens_cached,
        }
    }

    /// Get input price per million tokens for a model
    pub fn input_price_per_m(model: OpenAIModel) -> f64 {
        match model {
            OpenAIModel::Gpt51 => Self::GPT51_INPUT_PRICE_PER_M,
            OpenAIModel::Gpt51Mini => Self::GPT51_MINI_INPUT_PRICE_PER_M,
            OpenAIModel::Gpt51CodexMax => Self::GPT51_CODEX_MAX_INPUT_PRICE_PER_M,
        }
    }

    /// Get output price per million tokens for a model
    pub fn output_price_per_m(model: OpenAIModel) -> f64 {
        match model {
            OpenAIModel::Gpt51 => Self::GPT51_OUTPUT_PRICE_PER_M,
            OpenAIModel::Gpt51Mini => Self::GPT51_MINI_OUTPUT_PRICE_PER_M,
            OpenAIModel::Gpt51CodexMax => Self::GPT51_CODEX_MAX_OUTPUT_PRICE_PER_M,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gpt51_pricing() {
        // 100k input, 10k output
        // Input: 0.1 * $1.25 = $0.125
        // Output: 0.01 * $10 = $0.10
        // Total: $0.225
        let cost = OpenAIPricing::calculate_cost_gpt51(100_000, 10_000);
        assert!((cost - 0.225).abs() < 0.001);
    }

    #[test]
    fn test_gpt51_mini_pricing() {
        // 100k input, 10k output
        // Input: 0.1 * $0.25 = $0.025
        // Output: 0.01 * $2 = $0.02
        // Total: $0.045
        let cost = OpenAIPricing::calculate_cost_gpt51_mini(100_000, 10_000);
        assert!((cost - 0.045).abs() < 0.001);
    }

    #[test]
    fn test_cached_pricing() {
        // GPT-5.1: 100k total input, 50k cached, 10k output
        // Uncached input: 0.05 * $1.25 = $0.0625
        // Cached input: 0.05 * $0.125 = $0.00625
        // Output: 0.01 * $10 = $0.10
        // Total: $0.16875
        let cost = OpenAIPricing::calculate_cost_gpt51_with_cache(100_000, 10_000, 50_000);
        assert!((cost - 0.16875).abs() < 0.001);
    }

    #[test]
    fn test_cost_comparison() {
        // Same workload: 100k input, 10k output
        let gpt51_cost = OpenAIPricing::calculate_cost_gpt51(100_000, 10_000);
        let mini_cost = OpenAIPricing::calculate_cost_gpt51_mini(100_000, 10_000);

        // Mini should be ~5x cheaper
        assert!(mini_cost < gpt51_cost);
        assert!((gpt51_cost / mini_cost - 5.0).abs() < 0.1);
    }

    #[test]
    fn test_cost_with_info() {
        let result = OpenAIPricing::calculate_cost_with_info(
            OpenAIModel::Gpt51Mini,
            100_000,
            10_000,
            0,
        );
        assert_eq!(result.model, OpenAIModel::Gpt51Mini);
        assert_eq!(result.tokens_input, 100_000);
        assert_eq!(result.tokens_output, 10_000);
        assert!((result.cost - 0.045).abs() < 0.001);
    }
}
