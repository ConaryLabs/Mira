// crates/mira-server/src/hooks/precompact.rs
// PreCompact hook handler - preserves context before summarization

use crate::db::pool::DatabasePool;
use crate::db::{StoreObservationParams, store_observation_sync};
use crate::hooks::get_db_path;
use crate::utils::truncate_at_boundary;
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
        .and_then(|p| {
            let path = PathBuf::from(p);
            // Validate transcript_path is under user's home directory
            if let Some(home) = dirs::home_dir()
                && path.starts_with(&home)
            {
                return Some(path);
            }
            // Also allow /tmp which Claude Code may use
            if path.starts_with("/tmp") {
                return Some(path);
            }
            eprintln!(
                "[mira] PreCompact rejected transcript_path outside home directory: {}",
                p
            );
            None
        });

    eprintln!(
        "[mira] PreCompact hook triggered (session: {}, trigger: {})",
        truncate_at_boundary(session_id, 8),
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

    super::write_hook_output(&serde_json::json!({}));
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
    let project_id = crate::hooks::resolve_project_id(&pool).await;

    // Save compaction event as a session note
    let note_content = format!(
        "Context compaction ({}) triggered for session {}",
        trigger,
        truncate_at_boundary(session_id, 8)
    );

    // Store as a session event observation
    pool.interact(move |conn| {
        store_observation_sync(
            conn,
            StoreObservationParams {
                project_id,
                key: None,
                content: &note_content,
                observation_type: "session_event",
                category: Some("compaction"),
                confidence: COMPACTION_CONFIDENCE,
                source: "precompact",
                session_id: None,
                team_id: None,
                scope: "project",
                expires_at: Some("+7 days"),
            },
        )
        .map_err(|e| anyhow::anyhow!("{}", e))
    })
    .await?;

    // If we have transcript, extract key information
    if let Some(transcript) = transcript
        && let Err(e) = extract_and_save_context(&pool, project_id, session_id, transcript).await
    {
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

    // Filter and collect items to store
    let items: Vec<(String, String)> = important_lines
        .into_iter()
        .take(MAX_IMPORTANT_LINES)
        .filter(|(_, content)| content.len() >= MIN_CONTENT_LEN && content.len() <= MAX_CONTENT_LEN)
        .map(|(cat, content)| (cat.to_string(), content))
        .collect();

    let count = items.len();
    if count > 0 {
        let session_id_owned = session_id.to_string();
        // Batch all inserts into a single database interaction
        pool.interact(move |conn| {
            for (category, content) in &items {
                if let Err(e) = store_observation_sync(
                    conn,
                    StoreObservationParams {
                        project_id,
                        key: None,
                        content,
                        observation_type: "extracted",
                        category: Some(category),
                        confidence: 0.4,
                        source: "precompact",
                        session_id: Some(&session_id_owned),
                        team_id: None,
                        scope: "project",
                        expires_at: Some("+7 days"),
                    },
                ) {
                    eprintln!("[mira] Failed to store extracted context: {}", e);
                }
            }
            Ok::<_, anyhow::Error>(())
        })
        .await?;

        eprintln!("[mira] Extracted {} context items from transcript", count);
    }

    Ok(())
}
