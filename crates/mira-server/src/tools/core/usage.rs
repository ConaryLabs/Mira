// crates/mira-server/src/tools/core/usage.rs
// LLM usage analytics tool

use super::ToolContext;
use crate::db::{get_llm_usage_summary, query_llm_usage_stats};
use crate::error::MiraError;
use crate::utils::{format_period, truncate};

/// Get LLM usage summary
pub async fn usage_summary<C: ToolContext>(
    ctx: &C,
    since_days: Option<u32>,
) -> Result<String, MiraError> {
    let project_id = ctx.project_id().await;
    let since_days = since_days.or(Some(30));

    let stats = ctx
        .pool()
        .run(move |conn| get_llm_usage_summary(conn, project_id, since_days))
        .await?;

    let period = format_period(since_days);

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

/// Get LLM usage stats grouped by dimension
pub async fn usage_stats<C: ToolContext>(
    ctx: &C,
    group_by: Option<String>,
    since_days: Option<u32>,
    limit: Option<i64>,
) -> Result<String, MiraError> {
    let project_id = ctx.project_id().await;
    let since_days = since_days.or(Some(30));
    let limit = limit.unwrap_or(50).max(0) as usize;

    let group_by = group_by.unwrap_or_else(|| "role".to_string());
    let group_by_clone = group_by.clone();

    let all_stats = ctx
        .pool()
        .run(move |conn| query_llm_usage_stats(conn, &group_by_clone, project_id, since_days))
        .await?;

    if all_stats.is_empty() {
        return Ok("No usage data found. Usage data is recorded after MCP tool calls. Try using some tools first.".to_string());
    }

    let stats: Vec<_> = all_stats.into_iter().take(limit).collect();
    let period = format_period(since_days);

    let mut output = format!("LLM Usage by {} ({}, limit {})\n\n", group_by, period, limit);
    output.push_str(&format!(
        "{:<30} {:>8} {:>12} {:>10}\n",
        group_by.to_uppercase(),
        "REQUESTS",
        "TOKENS",
        "COST"
    ));
    output.push_str(&"-".repeat(65));
    output.push('\n');

    for stat in &stats {
        output.push_str(&format!(
            "{:<30} {:>8} {:>12} ${:>9.4}\n",
            truncate(&stat.group_key, 27),
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

/// List recent LLM usage records
pub async fn usage_list<C: ToolContext>(
    ctx: &C,
    since_days: Option<u32>,
    limit: Option<i64>,
) -> Result<String, MiraError> {
    let project_id = ctx.project_id().await;
    let since_days = since_days.or(Some(30));

    let stats = ctx
        .pool()
        .run(move |conn| query_llm_usage_stats(conn, "role", project_id, since_days))
        .await?;

    if stats.is_empty() {
        return Ok("No usage data found. Usage data is recorded after MCP tool calls. Try using some tools first.".to_string());
    }

    let period = format_period(since_days);
    let limit = limit.unwrap_or(50).max(0) as usize;

    let mut output = format!("Recent LLM Usage by Role ({}, limit {})\n\n", period, limit);

    for stat in stats.iter().take(limit) {
        output.push_str(&format!(
            "- {}: {} requests, {} tokens, ${:.4}\n",
            stat.group_key, stat.total_requests, stat.total_tokens, stat.total_cost
        ));
    }

    Ok(output)
}
