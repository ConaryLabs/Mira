// crates/mira-server/src/hooks/precompact.rs
// PreCompact hook handler - preserves context before summarization

use anyhow::Result;
use std::fs;
use std::path::PathBuf;

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
    // Get database
    let db_path = get_db_path();
    let db = std::sync::Arc::new(crate::db::Database::open(&db_path)?);

    // Get current project from last active
    let project_id = db
        .get_last_active_project()
        .ok()
        .flatten()
        .and_then(|path| db.get_or_create_project(&path, None).ok())
        .map(|(id, _)| id);

    // Save compaction event as a session note
    let note_content = format!(
        "Context compaction ({}) triggered for session {}",
        trigger,
        &session_id[..session_id.len().min(8)]
    );

    // Store as a session event
    db.store_memory(
        project_id,
        None,
        &note_content,
        "session_event",
        Some("compaction"),
        0.3, // Low confidence - just a log
    )?;

    // If we have transcript, extract key information
    if let Some(transcript) = transcript {
        if let Err(e) = extract_and_save_context(&db, project_id, session_id, transcript).await {
            eprintln!("[mira] Context extraction failed: {}", e);
        }
    }

    eprintln!("[mira] Pre-compaction state saved");
    Ok(())
}

/// Extract important context from transcript before it's summarized
async fn extract_and_save_context(
    db: &std::sync::Arc<crate::db::Database>,
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
            important_lines.push(("decision", line.trim()));
        }

        // Capture TODOs and next steps
        if lower.contains("todo:")
            || lower.contains("next step")
            || lower.contains("remaining:")
            || lower.contains("still need to")
        {
            important_lines.push(("context", line.trim()));
        }

        // Capture errors/issues
        if lower.contains("error:")
            || lower.contains("failed:")
            || lower.contains("issue:")
            || lower.contains("bug:")
        {
            important_lines.push(("issue", line.trim()));
        }
    }

    // Store extracted context
    let count = important_lines.len().min(10); // Limit to 10 items
    for (category, content) in important_lines.into_iter().take(10) {
        // Skip very short or very long lines
        if content.len() < 10 || content.len() > 500 {
            continue;
        }

        db.store_memory_with_session(
            project_id,
            None,
            content,
            "extracted",
            Some(category),
            0.4, // Moderate confidence - auto-extracted
            Some(session_id),
        )?;
    }

    if count > 0 {
        eprintln!("[mira] Extracted {} context items from transcript", count);
    }

    Ok(())
}
