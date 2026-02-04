// crates/mira-server/src/llm/pricing.rs
// LLM pricing configuration for cost estimation
//
// Pricing last updated: 2026-01-26
// Sources:
// - DeepSeek: https://api-docs.deepseek.com/quick_start/pricing
// - Gemini: https://ai.google.dev/gemini-api/docs/pricing
// - Z.AI (GLM): https://docs.z.ai/guides/overview/pricing

use super::{ChatResult, Provider};
use crate::db::pool::DatabasePool;
use crate::db::{LlmUsageRecord, insert_llm_usage_sync};
use std::sync::Arc;

/// Cost per million tokens (input, output)
#[derive(Debug, Clone, Copy)]
pub struct ModelPricing {
    /// Cost per 1M input tokens (cache miss)
    pub input_per_million: f64,
    /// Cost per 1M output tokens
    pub output_per_million: f64,
    /// Cost per 1M cached input tokens (if supported)
    pub cached_input_per_million: Option<f64>,
}

impl ModelPricing {
    const fn new(input: f64, output: f64) -> Self {
        Self {
            input_per_million: input,
            output_per_million: output,
            cached_input_per_million: None,
        }
    }

    const fn with_cache(input: f64, output: f64, cached: f64) -> Self {
        Self {
            input_per_million: input,
            output_per_million: output,
            cached_input_per_million: Some(cached),
        }
    }

    /// Calculate cost for a given usage
    pub fn calculate_cost(
        &self,
        prompt_tokens: u32,
        completion_tokens: u32,
        cache_hit_tokens: Option<u32>,
    ) -> f64 {
        let cache_hit = cache_hit_tokens.unwrap_or(0) as f64;
        let cache_miss = (prompt_tokens as f64) - cache_hit;

        let input_cost = if let Some(cached_price) = self.cached_input_per_million {
            // Provider supports caching - charge different rates
            (cache_hit * cached_price / 1_000_000.0)
                + (cache_miss * self.input_per_million / 1_000_000.0)
        } else {
            // No caching - all input at standard rate
            (prompt_tokens as f64) * self.input_per_million / 1_000_000.0
        };

        let output_cost = (completion_tokens as f64) * self.output_per_million / 1_000_000.0;

        input_cost + output_cost
    }
}

/// Get pricing for a provider/model combination
pub fn get_pricing(provider: Provider, model: &str) -> Option<ModelPricing> {
    match provider {
        Provider::DeepSeek => get_deepseek_pricing(model),
        Provider::Gemini => get_gemini_pricing(model),
        Provider::Ollama => Some(ModelPricing::new(0.0, 0.0)), // Local, no cost
        Provider::Sampling => Some(ModelPricing::new(0.0, 0.0)), // Host-provided, no direct cost
    }
}

/// DeepSeek pricing (as of 2026-01-26)
/// All models: $0.028/1M cache hit, $0.28/1M cache miss, $0.42/1M output
fn get_deepseek_pricing(model: &str) -> Option<ModelPricing> {
    match model {
        "deepseek-reasoner" | "deepseek-chat" => Some(ModelPricing::with_cache(0.28, 0.42, 0.028)),
        // Default for unknown DeepSeek models
        _ if model.starts_with("deepseek") => Some(ModelPricing::with_cache(0.28, 0.42, 0.028)),
        _ => None,
    }
}

/// Gemini pricing (as of 2026-01-26)
fn get_gemini_pricing(model: &str) -> Option<ModelPricing> {
    match model {
        // Gemini 3 Pro: $2.00/$12.00 (standard context <=200K)
        // Long context pricing ($4/$18) not tracked separately yet
        "gemini-3-pro-preview" | "gemini-3-pro" => Some(ModelPricing::new(2.00, 12.00)),
        // Gemini 3 Flash: $0.50/$3.00
        "gemini-3-flash" | "gemini-3-flash-preview" => Some(ModelPricing::new(0.50, 3.00)),
        _ => None,
    }
}

// ============================================================================
// Usage Recording Helper
// ============================================================================

/// Record LLM usage to database asynchronously
///
/// This is a fire-and-forget helper that logs usage without blocking the caller.
/// Call this after each LLM request to track costs.
pub async fn record_llm_usage(
    pool: &Arc<DatabasePool>,
    provider: Provider,
    model: &str,
    role: &str,
    result: &ChatResult,
    project_id: Option<i64>,
    session_id: Option<String>,
) {
    let Some(ref usage) = result.usage else {
        tracing::trace!(role, "No usage data in LLM response, skipping recording");
        return;
    };

    let record = LlmUsageRecord::new(
        provider,
        model,
        role,
        usage,
        result.duration_ms,
        project_id,
        session_id,
    );

    let pool = pool.clone();
    tokio::spawn(async move {
        if let Err(e) = pool
            .run(move |conn| insert_llm_usage_sync(conn, &record))
            .await
        {
            tracing::warn!(error = %e, "Failed to record LLM usage");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deepseek_pricing() {
        let pricing = get_pricing(Provider::DeepSeek, "deepseek-reasoner").unwrap();

        // 1M tokens each direction, no cache
        let cost = pricing.calculate_cost(1_000_000, 1_000_000, None);
        assert!((cost - 0.70).abs() < 0.01); // $0.28 input + $0.42 output

        // With 50% cache hit
        let cost = pricing.calculate_cost(1_000_000, 1_000_000, Some(500_000));
        // $0.014 (500K cached) + $0.14 (500K miss) + $0.42 (output) = $0.574
        assert!((cost - 0.574).abs() < 0.01);
    }

    #[test]
    fn test_gemini_pricing() {
        let pricing = get_pricing(Provider::Gemini, "gemini-3-pro-preview").unwrap();

        // 1M tokens each direction
        let cost = pricing.calculate_cost(1_000_000, 1_000_000, None);
        assert!((cost - 14.0).abs() < 0.01); // $2 input + $12 output
    }

    #[test]
    fn test_gemini_flash_pricing() {
        let pricing = get_pricing(Provider::Gemini, "gemini-3-flash").unwrap();

        // 1M tokens each direction
        let cost = pricing.calculate_cost(1_000_000, 1_000_000, None);
        assert!((cost - 3.50).abs() < 0.01); // $0.50 input + $3.00 output
    }

    #[test]
    fn test_ollama_free() {
        let pricing = get_pricing(Provider::Ollama, "llama3.3").unwrap();
        let cost = pricing.calculate_cost(1_000_000, 1_000_000, None);
        assert_eq!(cost, 0.0);
    }

    #[test]
    fn test_small_usage() {
        let pricing = get_pricing(Provider::DeepSeek, "deepseek-reasoner").unwrap();

        // 1000 tokens input, 500 output (typical small request)
        let cost = pricing.calculate_cost(1000, 500, None);
        // $0.28/1M * 1000 + $0.42/1M * 500 = $0.00028 + $0.00021 = $0.00049
        assert!((cost - 0.00049).abs() < 0.0001);
    }
}
