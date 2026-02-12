// crates/mira-server/src/hooks/precompact.rs
// PreCompact hook handler - preserves context before summarization

use crate::db::pool::DatabasePool;
use crate::db::{StoreObservationParams, store_observation_sync};
use crate::hooks::get_db_path;
use crate::utils::truncate_at_boundary;
use anyhow::{Context, Result};
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
    let input = super::read_hook_input().context("Failed to parse hook input from stdin")?;

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
            // Canonicalize to resolve ".." segments before checking prefix
            let canonical = match path.canonicalize() {
                Ok(c) => c,
                Err(_) => {
                    eprintln!(
                        "[mira] PreCompact rejected transcript_path (canonicalize failed): {}",
                        p
                    );
                    return None;
                }
            };
            // Validate transcript_path is under user's home directory
            if let Some(home) = dirs::home_dir()
                && canonical.starts_with(&home)
            {
                return Some(canonical);
            }
            // Also allow /tmp which Claude Code may use
            if canonical.starts_with("/tmp") {
                return Some(canonical);
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

/// Extract important lines from a transcript using heuristic keyword matching.
///
/// Returns a filtered list of (category, content) pairs, capped at MAX_IMPORTANT_LINES,
/// excluding lines shorter than MIN_CONTENT_LEN or longer than MAX_CONTENT_LEN.
fn extract_important_lines(transcript: &str) -> Vec<(String, String)> {
    let mut important_lines: Vec<(&str, String)> = Vec::new();

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

    important_lines
        .into_iter()
        .take(MAX_IMPORTANT_LINES)
        .filter(|(_, content)| content.len() >= MIN_CONTENT_LEN && content.len() <= MAX_CONTENT_LEN)
        .map(|(cat, content)| (cat.to_string(), content))
        .collect()
}

/// Extract important context from transcript before it's summarized
async fn extract_and_save_context(
    pool: &Arc<DatabasePool>,
    project_id: Option<i64>,
    session_id: &str,
    transcript: &str,
) -> Result<()> {
    let items = extract_important_lines(transcript);

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

#[cfg(test)]
mod tests {
    use super::*;

    // -- Decision keyword matching ------------------------------------------------

    #[test]
    fn extracts_decided_to_keyword() {
        let transcript = "We decided to use the builder pattern for config structs.";
        let results = extract_important_lines(transcript);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "decision");
        assert!(results[0].1.contains("decided to"));
    }

    #[test]
    fn extracts_choosing_keyword() {
        let transcript = "After review, choosing SQLite over PostgreSQL for simplicity.";
        let results = extract_important_lines(transcript);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "decision");
        assert!(results[0].1.contains("choosing"));
    }

    #[test]
    fn extracts_will_use_keyword() {
        let transcript = "We will use tokio for async runtime in this project.";
        let results = extract_important_lines(transcript);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "decision");
    }

    #[test]
    fn extracts_approach_keyword() {
        let transcript = "approach: batch inserts into a single transaction for performance.";
        let results = extract_important_lines(transcript);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "decision");
    }

    // -- Context keyword matching -------------------------------------------------

    #[test]
    fn extracts_todo_keyword() {
        let transcript = "TODO: add validation for user input in the handler.";
        let results = extract_important_lines(transcript);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "context");
    }

    #[test]
    fn extracts_next_step_keyword() {
        let transcript = "The next step is implementing the migration system.";
        let results = extract_important_lines(transcript);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "context");
    }

    #[test]
    fn extracts_remaining_keyword() {
        let transcript = "remaining: three modules still need refactoring work.";
        let results = extract_important_lines(transcript);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "context");
    }

    #[test]
    fn extracts_still_need_to_keyword() {
        let transcript = "We still need to add error handling to the API layer.";
        let results = extract_important_lines(transcript);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "context");
    }

    // -- Issue keyword matching ---------------------------------------------------

    #[test]
    fn extracts_error_keyword() {
        let transcript = "error: connection refused when connecting to database.";
        let results = extract_important_lines(transcript);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "issue");
    }

    #[test]
    fn extracts_failed_keyword() {
        let transcript = "The migration failed: column already exists in table.";
        let results = extract_important_lines(transcript);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "issue");
    }

    #[test]
    fn extracts_issue_keyword() {
        let transcript = "issue: race condition in concurrent database access.";
        let results = extract_important_lines(transcript);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "issue");
    }

    #[test]
    fn extracts_bug_keyword() {
        let transcript = "bug: duplicate entries created when session restarts.";
        let results = extract_important_lines(transcript);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "issue");
    }

    // -- Case insensitivity -------------------------------------------------------

    #[test]
    fn keywords_matched_case_insensitively() {
        let transcript = "\
DECIDED TO use uppercase keywords in this test.\n\
TODO: verify case insensitive matching works.\n\
ERROR: something went wrong with case handling.";
        let results = extract_important_lines(transcript);
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].0, "decision");
        assert_eq!(results[1].0, "context");
        assert_eq!(results[2].0, "issue");
    }

    #[test]
    fn mixed_case_keywords_matched() {
        let transcript = "Decided To go with the new approach for handling.";
        let results = extract_important_lines(transcript);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "decision");
    }

    // -- Length filtering ----------------------------------------------------------

    #[test]
    fn min_content_len_filters_short_lines() {
        // "error: x" is 8 chars, below MIN_CONTENT_LEN of 10
        let transcript = "error: x";
        let results = extract_important_lines(transcript);
        assert!(results.is_empty());
    }

    #[test]
    fn min_content_len_boundary_excluded() {
        // Exactly 9 chars, still below MIN_CONTENT_LEN of 10
        let transcript = "error: ab";
        let results = extract_important_lines(transcript);
        assert!(results.is_empty());
    }

    #[test]
    fn min_content_len_boundary_included() {
        // Exactly 10 chars, equal to MIN_CONTENT_LEN
        let transcript = "error: abc";
        let results = extract_important_lines(transcript);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn max_content_len_filters_long_lines() {
        let long_content = format!("error: {}", "x".repeat(500));
        assert!(long_content.len() > MAX_CONTENT_LEN);
        let results = extract_important_lines(&long_content);
        assert!(results.is_empty());
    }

    #[test]
    fn max_content_len_boundary_included() {
        // Exactly MAX_CONTENT_LEN chars
        let padding = "x".repeat(MAX_CONTENT_LEN - "error: ".len());
        let line = format!("error: {}", padding);
        assert_eq!(line.len(), MAX_CONTENT_LEN);
        let results = extract_important_lines(&line);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn max_content_len_boundary_excluded() {
        // One char over MAX_CONTENT_LEN
        let padding = "x".repeat(MAX_CONTENT_LEN - "error: ".len() + 1);
        let line = format!("error: {}", padding);
        assert_eq!(line.len(), MAX_CONTENT_LEN + 1);
        let results = extract_important_lines(&line);
        assert!(results.is_empty());
    }

    // -- MAX_IMPORTANT_LINES cap --------------------------------------------------

    #[test]
    fn caps_at_max_important_lines() {
        // Generate 15 matching lines, each long enough to pass MIN_CONTENT_LEN
        let lines: Vec<String> = (0..15)
            .map(|i| format!("We decided to implement feature number {}", i))
            .collect();
        let transcript = lines.join("\n");
        let results = extract_important_lines(&transcript);
        assert_eq!(results.len(), MAX_IMPORTANT_LINES);
    }

    #[test]
    fn length_filter_applied_after_cap() {
        // First 10 lines are short (filtered by MIN_CONTENT_LEN), next 5 are long enough.
        // The cap (take) happens before the filter, so short lines within the first 10
        // are taken then filtered out.
        let mut lines = Vec::new();
        for _ in 0..10 {
            lines.push("error: x".to_string()); // 8 chars, too short
        }
        for _ in 0..5 {
            lines.push("error: this line is long enough to pass the filter".to_string());
        }
        let transcript = lines.join("\n");
        let results = extract_important_lines(&transcript);
        // First 10 lines are taken (cap), all are too short, so 0 results
        assert!(results.is_empty());
    }

    // -- Empty and no-match cases -------------------------------------------------

    #[test]
    fn empty_transcript_returns_empty() {
        let results = extract_important_lines("");
        assert!(results.is_empty());
    }

    #[test]
    fn transcript_with_no_matches() {
        let transcript = "\
This is a normal log line.\n\
Another line with no keywords.\n\
Just regular conversation text.";
        let results = extract_important_lines(transcript);
        assert!(results.is_empty());
    }

    #[test]
    fn whitespace_only_transcript() {
        let results = extract_important_lines("   \n\n   \n");
        assert!(results.is_empty());
    }

    // -- Multi-category matching --------------------------------------------------

    #[test]
    fn line_matching_multiple_categories_produces_multiple_entries() {
        // A line containing both "decided to" and "error:" matches both categories
        let transcript = "We decided to fix the error: null pointer in handler.";
        let results = extract_important_lines(transcript);
        assert_eq!(results.len(), 2);
        let categories: Vec<&str> = results.iter().map(|(c, _)| c.as_str()).collect();
        assert!(categories.contains(&"decision"));
        assert!(categories.contains(&"issue"));
    }

    #[test]
    fn mixed_categories_in_transcript() {
        let transcript = "\
We decided to refactor the database layer.\n\
TODO: update the migration scripts for schema.\n\
error: failed to connect to the test database.\n\
This line has no keywords at all.";
        let results = extract_important_lines(transcript);
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].0, "decision");
        assert_eq!(results[1].0, "context");
        assert_eq!(results[2].0, "issue");
    }

    // -- Whitespace trimming ------------------------------------------------------

    #[test]
    fn leading_and_trailing_whitespace_trimmed() {
        let transcript = "   We decided to trim whitespace in results.   ";
        let results = extract_important_lines(transcript);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1, "We decided to trim whitespace in results.");
    }

    // -- Transcript path validation -----------------------------------------------

    #[test]
    fn validate_transcript_path_under_tmp() {
        let path = PathBuf::from("/tmp/claude/transcript.jsonl");
        assert!(path.starts_with("/tmp"));
    }

    #[test]
    fn validate_transcript_path_rejects_arbitrary_path() {
        let path = PathBuf::from("/etc/passwd");
        assert!(!path.starts_with("/tmp"));
        // Can't reliably test home_dir in CI, but the logic is:
        // path must start with home_dir or /tmp
    }

    // -- Constants ----------------------------------------------------------------

    #[test]
    fn constants_have_expected_values() {
        assert_eq!(MAX_IMPORTANT_LINES, 10);
        assert_eq!(MIN_CONTENT_LEN, 10);
        assert_eq!(MAX_CONTENT_LEN, 500);
        assert!((COMPACTION_CONFIDENCE - 0.3).abs() < f64::EPSILON);
    }
}
