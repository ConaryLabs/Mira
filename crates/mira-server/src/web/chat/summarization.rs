// web/chat/summarization.rs
// Rolling message summarization with multi-level promotion

use tracing::{info, warn};

use crate::web::deepseek::Message;
use crate::web::state::AppState;

/// Configuration for rolling summaries
const SUMMARY_WINDOW_SIZE: usize = 20;  // Keep this many recent messages unsummarized
const SUMMARY_BATCH_SIZE: usize = 10;   // Summarize this many messages at a time
const SUMMARY_THRESHOLD: usize = 30;    // Trigger summarization when this many unsummarized

// Multi-level summary promotion thresholds
const L1_PROMOTION_THRESHOLD: usize = 10; // Promote L1→L2 when this many session summaries
const L2_PROMOTION_THRESHOLD: usize = 7;  // Promote L2→L3 when this many daily summaries
const L1_PROMOTION_BATCH: usize = 5;      // Combine this many L1 summaries into one L2
const L2_PROMOTION_BATCH: usize = 5;      // Combine this many L2 summaries into one L3

/// Prompt for summarizing conversation chunks (L1 - session)
const SUMMARIZATION_PROMPT: &str = r#"Summarize this conversation segment concisely. Focus on:
- Key topics discussed
- Decisions made or preferences expressed
- Important context for future conversations
- Any action items or follow-ups mentioned

Keep it brief (2-4 sentences) but preserve important details.
Write in third person (e.g., "User discussed...", "They decided...")

Respond with ONLY the summary text, no preamble."#;

/// Prompt for combining summaries into higher-level summaries (L2/L3)
const PROMOTION_PROMPT: &str = r#"Combine these conversation summaries into a single higher-level summary.
Focus on the most important themes, decisions, and context that would be valuable long-term.
Be concise (2-3 sentences) but preserve key information.

Respond with ONLY the combined summary text, no preamble."#;

/// Check if we need to summarize and spawn background task if so
pub fn maybe_spawn_summarization(state: AppState) {
    tokio::spawn(async move {
        // Get project_id for project-scoped summaries
        let project_id = state.project_id().await;

        // Check message count for L1 summarization
        let count = match state.db.count_unsummarized_messages() {
            Ok(c) => c as usize,
            Err(_) => return,
        };

        if count >= SUMMARY_THRESHOLD {
            info!("Triggering rolling summarization: {} unsummarized messages", count);
            if let Err(e) = perform_rolling_summarization(&state, project_id).await {
                warn!("Rolling summarization failed: {}", e);
            }
        }

        // Check for L1→L2 promotion
        let l1_count = state.db.count_summaries_at_level(project_id, 1).unwrap_or(0) as usize;
        if l1_count >= L1_PROMOTION_THRESHOLD {
            info!("Triggering L1→L2 promotion: {} session summaries", l1_count);
            if let Err(e) = promote_summaries(&state, project_id, 1, 2, L1_PROMOTION_BATCH).await {
                warn!("L1→L2 promotion failed: {}", e);
            }
        }

        // Check for L2→L3 promotion
        let l2_count = state.db.count_summaries_at_level(project_id, 2).unwrap_or(0) as usize;
        if l2_count >= L2_PROMOTION_THRESHOLD {
            info!("Triggering L2→L3 promotion: {} daily summaries", l2_count);
            if let Err(e) = promote_summaries(&state, project_id, 2, 3, L2_PROMOTION_BATCH).await {
                warn!("L2→L3 promotion failed: {}", e);
            }
        }
    });
}

/// Perform rolling summarization of older messages
async fn perform_rolling_summarization(
    state: &AppState,
    project_id: Option<i64>,
) -> anyhow::Result<()> {
    let deepseek = state.deepseek.as_ref()
        .ok_or_else(|| anyhow::anyhow!("DeepSeek not configured"))?;

    // Get the oldest unsummarized messages (beyond our window)
    // First, get recent messages to find the cutoff point
    let recent = state.db.get_recent_messages(SUMMARY_WINDOW_SIZE)?;

    if recent.is_empty() {
        return Ok(());
    }

    // Get oldest message ID in our "keep" window
    let oldest_kept_id = recent.first().map(|m| m.id).unwrap_or(0);

    // Get messages before that for summarization
    let to_summarize = state.db.get_messages_before(oldest_kept_id, SUMMARY_BATCH_SIZE)?;

    if to_summarize.is_empty() {
        return Ok(());
    }

    let start_id = to_summarize.first().unwrap().id;
    let end_id = to_summarize.last().unwrap().id;

    info!(
        "Summarizing messages {} to {} ({} messages) for project {:?}",
        start_id, end_id, to_summarize.len(), project_id
    );

    // Format messages for summarization
    let conversation_text: String = to_summarize
        .iter()
        .map(|m| format!("{}: {}", m.role.to_uppercase(), m.content))
        .collect::<Vec<_>>()
        .join("\n\n");

    // Call DeepSeek to summarize
    let messages = vec![
        Message::system(SUMMARIZATION_PROMPT.to_string()),
        Message::user(conversation_text),
    ];

    let result = deepseek.chat(messages, None).await?;

    let summary = result.content
        .or(result.reasoning_content)
        .ok_or_else(|| anyhow::anyhow!("No summary generated"))?;

    // Store summary with project scope
    let summary_id = state.db.store_chat_summary(project_id, &summary, start_id, end_id, 1)?;
    info!("Stored summary {} covering messages {}-{}", summary_id, start_id, end_id);

    // Mark messages as summarized
    let marked = state.db.mark_messages_summarized(start_id, end_id)?;
    info!("Marked {} messages as summarized", marked);

    Ok(())
}

/// Promote summaries from one level to the next
async fn promote_summaries(
    state: &AppState,
    project_id: Option<i64>,
    from_level: i32,
    to_level: i32,
    batch_size: usize,
) -> anyhow::Result<()> {
    let deepseek = state.deepseek.as_ref()
        .ok_or_else(|| anyhow::anyhow!("DeepSeek not configured"))?;

    // Get oldest summaries at the source level for this project
    let summaries = state.db.get_oldest_summaries(project_id, from_level, batch_size)?;

    if summaries.is_empty() {
        return Ok(());
    }

    let ids: Vec<i64> = summaries.iter().map(|s| s.id).collect();
    let range_start = summaries.first().unwrap().message_range_start;
    let range_end = summaries.last().unwrap().message_range_end;

    info!(
        "Promoting {} L{} summaries to L{} (covering {}-{}) for project {:?}",
        summaries.len(), from_level, to_level, range_start, range_end, project_id
    );

    // Combine summaries for the LLM
    let combined_text: String = summaries
        .iter()
        .map(|s| format!("- {}", s.summary))
        .collect::<Vec<_>>()
        .join("\n");

    // Call DeepSeek to create higher-level summary
    let messages = vec![
        Message::system(PROMOTION_PROMPT.to_string()),
        Message::user(combined_text),
    ];

    let result = deepseek.chat(messages, None).await?;

    let new_summary = result.content
        .or(result.reasoning_content)
        .ok_or_else(|| anyhow::anyhow!("No promoted summary generated"))?;

    // Store the new higher-level summary with project scope
    let new_id = state.db.store_chat_summary(project_id, &new_summary, range_start, range_end, to_level)?;
    info!("Created L{} summary {} from {} L{} summaries", to_level, new_id, ids.len(), from_level);

    // Delete the old summaries
    let deleted = state.db.delete_summaries(&ids)?;
    info!("Deleted {} promoted L{} summaries", deleted, from_level);

    Ok(())
}

/// Get recent summaries for context injection (all levels)
pub fn get_summary_context(
    db: &crate::db::Database,
    project_id: Option<i64>,
    limit: usize,
) -> String {
    let mut parts = Vec::new();

    // L3 - Weekly summaries (oldest context, most compressed)
    if let Ok(summaries) = db.get_recent_summaries(project_id, 3, 2) {
        if !summaries.is_empty() {
            parts.push("Long-term context:".to_string());
            for s in summaries {
                parts.push(format!("  - {}", s.summary));
            }
        }
    }

    // L2 - Daily summaries
    if let Ok(summaries) = db.get_recent_summaries(project_id, 2, 3) {
        if !summaries.is_empty() {
            parts.push("Recent days:".to_string());
            for s in summaries {
                parts.push(format!("  - {}", s.summary));
            }
        }
    }

    // L1 - Session summaries (most recent compressed context)
    if let Ok(summaries) = db.get_recent_summaries(project_id, 1, limit) {
        if !summaries.is_empty() {
            parts.push("Earlier today:".to_string());
            for s in summaries {
                parts.push(format!("  - {}", s.summary));
            }
        }
    }

    parts.join("\n")
}
