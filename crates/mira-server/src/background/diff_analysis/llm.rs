// background/diff_analysis/llm.rs
// LLM-powered semantic diff analysis

use super::types::{LlmDiffResponse, SemanticChange};
use crate::db::pool::DatabasePool;
use crate::llm::{LlmClient, PromptBuilder, chat_with_usage};
use crate::utils::json::parse_json_hardened;
use std::sync::Arc;

/// Maximum diff size to send to LLM (in bytes)
const MAX_DIFF_SIZE: usize = 50_000;

/// Analyze diff semantically using LLM
pub async fn analyze_diff_semantic(
    diff_content: &str,
    llm_client: &Arc<dyn LlmClient>,
    pool: &Arc<DatabasePool>,
    project_id: Option<i64>,
) -> Result<(Vec<SemanticChange>, String, Vec<String>), String> {
    if diff_content.is_empty() {
        return Ok((Vec::new(), "No changes".to_string(), Vec::new()));
    }

    // Truncate if too large
    let diff_to_analyze = if diff_content.len() > MAX_DIFF_SIZE {
        format!(
            "{}...\n\n[Diff truncated - {} more bytes]",
            &diff_content[..MAX_DIFF_SIZE],
            diff_content.len() - MAX_DIFF_SIZE
        )
    } else {
        diff_content.to_string()
    };

    let user_prompt = format!(
        "Analyze this git diff:\n\n```diff\n{}\n```",
        diff_to_analyze
    );

    let messages = PromptBuilder::for_diff_analysis().build_messages(user_prompt);

    let content = chat_with_usage(
        &**llm_client,
        pool,
        messages,
        "background:diff_analysis",
        project_id,
        None,
    )
    .await?;

    // Try to parse JSON from response
    parse_llm_response(&content)
}

/// Parse the LLM response to extract structured data
pub(super) fn parse_llm_response(
    content: &str,
) -> Result<(Vec<SemanticChange>, String, Vec<String>), String> {
    // Try hardened JSON parsing first
    if let Ok(response) = parse_json_hardened::<LlmDiffResponse>(content) {
        return Ok((response.changes, response.summary, response.risk_flags));
    }

    // Fallback: extract what we can from plain text
    let summary = content
        .lines()
        .find(|l| !l.trim().is_empty() && !l.starts_with('{'))
        .unwrap_or("Changes analyzed")
        .to_string();

    Ok((Vec::new(), summary, Vec::new()))
}
