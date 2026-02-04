// crates/mira-server/src/hooks/precompact.rs
// PreCompact hook handler - preserves context before summarization

use crate::db::pool::DatabasePool;
use crate::db::{
    StoreMemoryParams, get_last_active_project_sync, get_or_create_project_sync, store_memory_sync,
};
use anyhow::Result;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

/// Confidence level for compaction log entries
const COMPACTION_CONFIDENCE: f64 = 0.3;
/// Maximum number of important lines to extract from a transcript
const MAX_IMPORTANT_LINES: usize = 10;
/// Minimum content length for extracted lines (skip trivial entries)
const MIN_CONTENT_LEN: usize = 10;
/// Maximum content length for extracted lines (skip code pastes)
const MAX_CONTENT_LEN: usize = 500;

/// Get database path (same as main.rs)
fn get_db_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".mira/mira.db")
}

/// Handle PreCompact hook from Claude Code
/// Fires before context compaction (summarization) occurs
/// Input: { session_id, transcript_path, trigger: "manual"|"auto", custom_instructions }
pub async fn run() -> Result<()> {
    let input = super::read_hook_input()?;

    let session_id = input
        .get("session_id")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let trigger = input
        .get("trigger")
        .and_then(|v| v.as_str())
        .unwrap_or("auto");
    let transcript_path = input
        .get("transcript_path")
        .and_then(|v| v.as_str())
        .map(PathBuf::from);

    eprintln!(
        "[mira] PreCompact hook triggered (session: {}, trigger: {})",
        &session_id[..session_id.len().min(8)],
        trigger
    );

    // Read transcript if available
    let transcript = transcript_path
        .as_ref()
        .and_then(|p| fs::read_to_string(p).ok());

    // Save pre-compaction state
    if let Err(e) = save_pre_compaction_state(session_id, trigger, transcript.as_deref()).await {
        eprintln!("[mira] Failed to save pre-compaction state: {}", e);
    }

    Ok(())
}

/// Save important context before compaction occurs
async fn save_pre_compaction_state(
    session_id: &str,
    trigger: &str,
    transcript: Option<&str>,
) -> Result<()> {
    // Get database pool
    let db_path = get_db_path();
    let pool = Arc::new(DatabasePool::open(&db_path).await?);

    // Get current project from last active
    let project_id = {
        let pool_clone = pool.clone();
        pool_clone
            .interact(move |conn| {
                let path = get_last_active_project_sync(conn).ok().flatten();
                let result = if let Some(path) = path {
                    get_or_create_project_sync(conn, &path, None)
                        .ok()
                        .map(|(id, _)| id)
                } else {
                    None
                };
                Ok::<_, anyhow::Error>(result)
            })
            .await
            .ok()
            .flatten()
    };

    // Save compaction event as a session note
    let note_content = format!(
        "Context compaction ({}) triggered for session {}",
        trigger,
        &session_id[..session_id.len().min(8)]
    );

    // Store as a session event
    {
        let pool_clone = pool.clone();
        pool_clone
            .interact(move |conn| {
                store_memory_sync(
                    conn,
                    StoreMemoryParams {
                        project_id,
                        key: None,
                        content: &note_content,
                        fact_type: "session_event",
                        category: Some("compaction"),
                        confidence: COMPACTION_CONFIDENCE,
                        session_id: None,
                        user_id: None,
                        scope: "project",
                        branch: None,
                    },
                )
                .map_err(|e| anyhow::anyhow!("{}", e))
            })
            .await?
    };

    // If we have transcript, extract key information
    if let Some(transcript) = transcript
        && let Err(e) = extract_and_save_context(&pool, project_id, session_id, transcript).await {
            eprintln!("[mira] Context extraction failed: {}", e);
        }

    eprintln!("[mira] Pre-compaction state saved");
    Ok(())
}

/// Extract important context from transcript before it's summarized
async fn extract_and_save_context(
    pool: &Arc<DatabasePool>,
    project_id: Option<i64>,
    session_id: &str,
    transcript: &str,
) -> Result<()> {
    // Simple heuristic extraction - look for patterns that indicate important info
    let mut important_lines = Vec::new();

    for line in transcript.lines() {
        let lower = line.to_lowercase();

        // Capture decisions
        if lower.contains("decided to")
            || lower.contains("choosing")
            || lower.contains("will use")
            || lower.contains("approach:")
        {
            important_lines.push(("decision", line.trim().to_string()));
        }

        // Capture TODOs and next steps
        if lower.contains("todo:")
            || lower.contains("next step")
            || lower.contains("remaining:")
            || lower.contains("still need to")
        {
            important_lines.push(("context", line.trim().to_string()));
        }

        // Capture errors/issues
        if lower.contains("error:")
            || lower.contains("failed:")
            || lower.contains("issue:")
            || lower.contains("bug:")
        {
            important_lines.push(("issue", line.trim().to_string()));
        }
    }

    // Store extracted context
    let count = important_lines.len().min(MAX_IMPORTANT_LINES);
    let session_id_owned = session_id.to_string();

    for (category, content) in important_lines.into_iter().take(MAX_IMPORTANT_LINES) {
        // Skip very short or very long lines
        if content.len() < MIN_CONTENT_LEN || content.len() > MAX_CONTENT_LEN {
            continue;
        }

        let category_owned = category.to_string();
        let session_id_clone = session_id_owned.clone();
        pool.interact(move |conn| {
            store_memory_sync(
                conn,
                StoreMemoryParams {
                    project_id,
                    key: None,
                    content: &content,
                    fact_type: "extracted",
                    category: Some(&category_owned),
                    confidence: 0.4, // Moderate confidence - auto-extracted
                    session_id: Some(&session_id_clone),
                    user_id: None,
                    scope: "project",
                    branch: None,
                },
            )
            .map_err(|e| anyhow::anyhow!("{}", e))
        })
        .await?;
    }

    if count > 0 {
        eprintln!("[mira] Extracted {} context items from transcript", count);
    }

    Ok(())
}
