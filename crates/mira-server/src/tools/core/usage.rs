// tools/core/usage.rs
// LLM usage analytics tool

use super::ToolContext;
use crate::db::{query_llm_usage_stats, get_llm_usage_summary};
use crate::mcp::requests::UsageAction;

/// Query LLM usage statistics
pub async fn usage<C: ToolContext>(
    ctx: &C,
    action: UsageAction,
    group_by: Option<String>,
    since_days: Option<u32>,
    limit: Option<i64>,
) -> Result<String, String> {
    let project_id = ctx.project_id().await;
    let since_days = since_days.or(Some(30)); // Default to last 30 days

    match action {
        UsageAction::Summary => {
            let stats = ctx
                .pool()
                .run(move |conn| get_llm_usage_summary(conn, project_id, since_days))
                .await?;

            let period = since_days.map(|d| format!("last {} days", d)).unwrap_or_else(|| "all time".to_string());

            Ok(format!(
                "LLM Usage Summary ({})\n\n\
                 Requests: {}\n\
                 Total tokens: {} ({} prompt + {} completion)\n\
                 Estimated cost: ${:.4}\n\
                 Avg duration: {:.0}ms",
                period,
                stats.total_requests,
                stats.total_tokens,
                stats.prompt_tokens,
                stats.completion_tokens,
                stats.total_cost,
                stats.avg_duration_ms.unwrap_or(0.0)
            ))
        }

        UsageAction::Stats => {
            let group_by = group_by.unwrap_or_else(|| "role".to_string());
            let group_by_clone = group_by.clone();

            let stats = ctx
                .pool()
                .run(move |conn| query_llm_usage_stats(conn, &group_by_clone, project_id, since_days))
                .await?;

            if stats.is_empty() {
                return Ok("No usage data found".to_string());
            }

            let period = since_days.map(|d| format!("last {} days", d)).unwrap_or_else(|| "all time".to_string());

            let mut output = format!("LLM Usage by {} ({})\n\n", group_by, period);
            output.push_str(&format!(
                "{:<30} {:>8} {:>12} {:>10}\n",
                group_by.to_uppercase(), "REQUESTS", "TOKENS", "COST"
            ));
            output.push_str(&"-".repeat(65));
            output.push('\n');

            for stat in &stats {
                output.push_str(&format!(
                    "{:<30} {:>8} {:>12} ${:>9.4}\n",
                    truncate_str(&stat.group_key, 30),
                    stat.total_requests,
                    stat.total_tokens,
                    stat.total_cost
                ));
            }

            // Add total
            let total_requests: u64 = stats.iter().map(|s| s.total_requests).sum();
            let total_tokens: u64 = stats.iter().map(|s| s.total_tokens).sum();
            let total_cost: f64 = stats.iter().map(|s| s.total_cost).sum();

            output.push_str(&"-".repeat(65));
            output.push('\n');
            output.push_str(&format!(
                "{:<30} {:>8} {:>12} ${:>9.4}\n",
                "TOTAL", total_requests, total_tokens, total_cost
            ));

            Ok(output)
        }

        UsageAction::List => {
            // For now, list just shows stats by role - we could add a detailed list later
            let stats = ctx
                .pool()
                .run(move |conn| query_llm_usage_stats(conn, "role", project_id, since_days))
                .await?;

            if stats.is_empty() {
                return Ok("No usage data found".to_string());
            }

            let period = since_days.map(|d| format!("last {} days", d)).unwrap_or_else(|| "all time".to_string());
            let limit = limit.unwrap_or(50) as usize;

            let mut output = format!("Recent LLM Usage by Role ({}, limit {})\n\n", period, limit);

            for stat in stats.iter().take(limit) {
                output.push_str(&format!(
                    "- {}: {} requests, {} tokens, ${:.4}\n",
                    stat.group_key,
                    stat.total_requests,
                    stat.total_tokens,
                    stat.total_cost
                ));
            }

            Ok(output)
        }
    }
}

fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}
