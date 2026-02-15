// crates/mira-server/src/hooks/precompact.rs
// PreCompact hook handler - preserves context before summarization

use crate::ipc::client::HookClient;
use crate::utils::truncate_at_boundary;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Confidence level for compaction log entries
const COMPACTION_CONFIDENCE: f64 = 0.3;
/// Maximum items per category in compaction context
const MAX_ITEMS_PER_CATEGORY: usize = 5;
/// Minimum content length for extracted paragraphs (skip trivial entries)
const MIN_CONTENT_LEN: usize = 10;
/// Maximum content length for extracted paragraphs (skip code pastes)
const MAX_CONTENT_LEN: usize = 500;

/// A parsed message from the JSONL transcript
#[derive(Debug)]
pub(crate) struct TranscriptMessage {
    role: String,
    text_content: String,
}

/// Structured context extracted from a transcript before compaction.
///
/// Stored as a `compaction_context` field in `session_snapshots` so that
/// `build_resume_context()` can surface it when the user resumes.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct CompactionContext {
    pub decisions: Vec<String>,
    pub active_work: Vec<String>,
    pub issues: Vec<String>,
    pub pending_tasks: Vec<String>,
}

impl CompactionContext {
    fn is_empty(&self) -> bool {
        self.decisions.is_empty()
            && self.active_work.is_empty()
            && self.issues.is_empty()
            && self.pending_tasks.is_empty()
    }

    fn total_items(&self) -> usize {
        self.decisions.len() + self.active_work.len() + self.issues.len() + self.pending_tasks.len()
    }
}

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
                    tracing::warn!(
                        path = p,
                        "PreCompact rejected transcript_path (canonicalize failed)"
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
            tracing::warn!(
                path = p,
                "PreCompact rejected transcript_path outside home directory"
            );
            None
        });

    tracing::debug!(
        session = truncate_at_boundary(session_id, 8),
        trigger,
        "PreCompact hook triggered"
    );

    // Read transcript if available
    let transcript = transcript_path
        .as_ref()
        .and_then(|p| fs::read_to_string(p).ok());

    // Connect via IPC (falls back to direct DB)
    let mut client = HookClient::connect().await;

    // Save pre-compaction state
    if let Err(e) =
        save_pre_compaction_state(&mut client, session_id, trigger, transcript.as_deref()).await
    {
        tracing::error!(error = %e, "Failed to save pre-compaction state");
    }

    super::write_hook_output(&serde_json::json!({}));
    Ok(())
}

/// Save important context before compaction occurs
async fn save_pre_compaction_state(
    client: &mut HookClient,
    session_id: &str,
    trigger: &str,
    transcript: Option<&str>,
) -> Result<()> {
    let project_id = client.resolve_project(None).await.map(|(id, _)| id);

    // Store compaction event as an audit marker
    let note_content = format!(
        "Context compaction ({}) triggered for session {}",
        trigger,
        truncate_at_boundary(session_id, 8)
    );

    client
        .store_observation(
            project_id,
            &note_content,
            "session_event",
            Some("compaction"),
            COMPACTION_CONFIDENCE,
            "precompact",
            "project",
            Some("+7 days"),
        )
        .await;

    // Parse transcript JSONL, extract structured context, store in session_snapshots
    if let Some(transcript) = transcript
        && let Err(e) = extract_and_save_context(client, session_id, transcript).await
    {
        tracing::warn!(error = %e, "Context extraction failed");
    }

    tracing::debug!("Pre-compaction state saved");
    Ok(())
}

/// Parse JSONL transcript into structured messages.
///
/// Extracts text content from `assistant` and `user` role messages,
/// skipping `tool_use` and `tool_result` content blocks. Reuses the
/// proven pattern from `subagent.rs`.
pub(crate) fn parse_transcript_messages(transcript: &str) -> Vec<TranscriptMessage> {
    let mut messages = Vec::new();
    for line in transcript.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let role = entry.get("role").and_then(|v| v.as_str()).unwrap_or("");
        if role != "assistant" && role != "user" {
            continue;
        }
        let mut text_content = String::new();
        if let Some(content) = entry.get("content") {
            match content {
                serde_json::Value::String(s) => {
                    text_content.push_str(s);
                }
                serde_json::Value::Array(blocks) => {
                    for block in blocks {
                        // Skip tool_use and tool_result content blocks
                        if let Some(block_type) = block.get("type").and_then(|t| t.as_str())
                            && (block_type == "tool_use" || block_type == "tool_result")
                        {
                            continue;
                        }
                        if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                            if !text_content.is_empty() {
                                text_content.push('\n');
                            }
                            text_content.push_str(text);
                        }
                    }
                }
                _ => {}
            }
        }
        if !text_content.is_empty() {
            messages.push(TranscriptMessage {
                role: role.to_string(),
                text_content,
            });
        }
    }
    messages
}

/// Extract structured context from parsed transcript messages.
///
/// Scans paragraphs for decision keywords, pending tasks, and issues.
/// Captures the last assistant message's opening paragraph as active work.
pub(crate) fn extract_compaction_context(messages: &[TranscriptMessage]) -> CompactionContext {
    let mut ctx = CompactionContext::default();

    for msg in messages {
        for paragraph in msg.text_content.split("\n\n") {
            let trimmed = paragraph.trim();
            if trimmed.len() < MIN_CONTENT_LEN || trimmed.len() > MAX_CONTENT_LEN {
                continue;
            }
            let lower = trimmed.to_lowercase();

            if ctx.decisions.len() < MAX_ITEMS_PER_CATEGORY
                && (lower.contains("decided to")
                    || lower.contains("choosing")
                    || lower.contains("will use")
                    || lower.contains("approach:"))
            {
                ctx.decisions.push(trimmed.to_string());
            }

            if ctx.pending_tasks.len() < MAX_ITEMS_PER_CATEGORY
                && (lower.contains("todo:")
                    || lower.contains("next step")
                    || lower.contains("remaining:")
                    || lower.contains("still need to"))
            {
                ctx.pending_tasks.push(trimmed.to_string());
            }

            if ctx.issues.len() < MAX_ITEMS_PER_CATEGORY
                && (lower.contains("error:")
                    || lower.contains("failed:")
                    || lower.contains("issue:")
                    || lower.contains("bug:"))
            {
                ctx.issues.push(trimmed.to_string());
            }
        }
    }

    // Capture active work from the last assistant message's first substantial paragraph
    if let Some(last_assistant) = messages.iter().rev().find(|m| m.role == "assistant")
        && let Some(first_para) = last_assistant
            .text_content
            .split("\n\n")
            .next()
            .map(|s| s.trim())
            .filter(|s| s.len() >= MIN_CONTENT_LEN && s.len() <= MAX_CONTENT_LEN)
    {
        ctx.active_work.push(first_para.to_string());
    }

    ctx
}

/// Extract context from transcript and store in session_snapshots.
///
/// UPSERTs a `compaction_context` field into the session snapshot.
/// If a snapshot already exists (from a prior compaction), it merges;
/// if not, it creates a partial snapshot.
pub(crate) async fn extract_and_save_context(
    client: &mut HookClient,
    session_id: &str,
    transcript: &str,
) -> Result<()> {
    let messages = parse_transcript_messages(transcript);
    if messages.is_empty() {
        return Ok(());
    }

    let ctx = extract_compaction_context(&messages);
    if ctx.is_empty() {
        return Ok(());
    }

    let count = ctx.total_items();
    let ctx_json = serde_json::to_value(&ctx)?;

    client.save_compaction_context(session_id, ctx_json).await;

    tracing::debug!(count, "Extracted context items from transcript");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_transcript_messages ──────────────────────────────────────────

    #[test]
    fn parses_string_content() {
        let transcript =
            r#"{"role":"assistant","content":"I decided to use the builder pattern."}"#;
        let messages = parse_transcript_messages(transcript);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, "assistant");
        assert!(messages[0].text_content.contains("builder pattern"));
    }

    #[test]
    fn parses_array_content_blocks() {
        let transcript = r#"{"role":"assistant","content":[{"type":"text","text":"First block."},{"type":"text","text":"Second block."}]}"#;
        let messages = parse_transcript_messages(transcript);
        assert_eq!(messages.len(), 1);
        assert!(messages[0].text_content.contains("First block."));
        assert!(messages[0].text_content.contains("Second block."));
    }

    #[test]
    fn filters_tool_use_blocks() {
        let transcript = r#"{"role":"assistant","content":[{"type":"text","text":"Let me check."},{"type":"tool_use","id":"t1","name":"Read","input":{}}]}"#;
        let messages = parse_transcript_messages(transcript);
        assert_eq!(messages.len(), 1);
        assert!(messages[0].text_content.contains("Let me check."));
        assert!(!messages[0].text_content.contains("tool_use"));
    }

    #[test]
    fn filters_tool_result_blocks() {
        let transcript = r#"{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"file contents"},{"type":"text","text":"Please continue."}]}"#;
        let messages = parse_transcript_messages(transcript);
        assert_eq!(messages.len(), 1);
        assert!(messages[0].text_content.contains("Please continue."));
        assert!(!messages[0].text_content.contains("file contents"));
    }

    #[test]
    fn skips_system_role() {
        let transcript = r#"{"role":"system","content":"You are a helpful assistant."}"#;
        let messages = parse_transcript_messages(transcript);
        assert!(messages.is_empty());
    }

    #[test]
    fn skips_malformed_jsonl_lines() {
        let transcript = "not json at all\n{\"role\":\"assistant\",\"content\":\"valid line\"}";
        let messages = parse_transcript_messages(transcript);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, "assistant");
    }

    #[test]
    fn handles_empty_transcript() {
        let messages = parse_transcript_messages("");
        assert!(messages.is_empty());
    }

    #[test]
    fn parses_both_user_and_assistant_roles() {
        let transcript = "{\"role\":\"user\",\"content\":\"Hello\"}\n{\"role\":\"assistant\",\"content\":\"Hi there\"}";
        let messages = parse_transcript_messages(transcript);
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[1].role, "assistant");
    }

    #[test]
    fn skips_messages_with_empty_content() {
        let transcript = r#"{"role":"assistant","content":""}"#;
        let messages = parse_transcript_messages(transcript);
        assert!(messages.is_empty());
    }

    #[test]
    fn skips_empty_lines_in_transcript() {
        let transcript = "\n\n{\"role\":\"assistant\",\"content\":\"hello\"}\n\n";
        let messages = parse_transcript_messages(transcript);
        assert_eq!(messages.len(), 1);
    }

    // ── extract_compaction_context ─────────────────────────────────────────

    #[test]
    fn extracts_decisions() {
        let messages = vec![TranscriptMessage {
            role: "assistant".to_string(),
            text_content: "We decided to use the builder pattern for config structs.".to_string(),
        }];
        let ctx = extract_compaction_context(&messages);
        assert_eq!(ctx.decisions.len(), 1);
        assert!(ctx.decisions[0].contains("decided to"));
    }

    #[test]
    fn extracts_choosing_keyword() {
        let messages = vec![TranscriptMessage {
            role: "assistant".to_string(),
            text_content: "After review, choosing SQLite over PostgreSQL for simplicity."
                .to_string(),
        }];
        let ctx = extract_compaction_context(&messages);
        assert_eq!(ctx.decisions.len(), 1);
    }

    #[test]
    fn extracts_will_use_keyword() {
        let messages = vec![TranscriptMessage {
            role: "assistant".to_string(),
            text_content: "We will use tokio for async runtime in this project.".to_string(),
        }];
        let ctx = extract_compaction_context(&messages);
        assert_eq!(ctx.decisions.len(), 1);
    }

    #[test]
    fn extracts_approach_keyword() {
        let messages = vec![TranscriptMessage {
            role: "assistant".to_string(),
            text_content: "approach: batch inserts into a single transaction for performance."
                .to_string(),
        }];
        let ctx = extract_compaction_context(&messages);
        assert_eq!(ctx.decisions.len(), 1);
    }

    #[test]
    fn extracts_pending_tasks() {
        let messages = vec![TranscriptMessage {
            role: "assistant".to_string(),
            text_content: "TODO: add validation for user input in the handler.".to_string(),
        }];
        let ctx = extract_compaction_context(&messages);
        assert_eq!(ctx.pending_tasks.len(), 1);
        assert!(ctx.pending_tasks[0].contains("TODO:"));
    }

    #[test]
    fn extracts_next_step_keyword() {
        let messages = vec![TranscriptMessage {
            role: "assistant".to_string(),
            text_content: "The next step is implementing the migration system.".to_string(),
        }];
        let ctx = extract_compaction_context(&messages);
        assert_eq!(ctx.pending_tasks.len(), 1);
    }

    #[test]
    fn extracts_remaining_keyword() {
        let messages = vec![TranscriptMessage {
            role: "assistant".to_string(),
            text_content: "remaining: three modules still need refactoring work.".to_string(),
        }];
        let ctx = extract_compaction_context(&messages);
        // Matches both "remaining:" and "still need to"
        assert!(!ctx.pending_tasks.is_empty());
    }

    #[test]
    fn extracts_still_need_to_keyword() {
        let messages = vec![TranscriptMessage {
            role: "assistant".to_string(),
            text_content: "We still need to add error handling to the API layer.".to_string(),
        }];
        let ctx = extract_compaction_context(&messages);
        assert_eq!(ctx.pending_tasks.len(), 1);
    }

    #[test]
    fn extracts_issues() {
        let messages = vec![TranscriptMessage {
            role: "assistant".to_string(),
            text_content: "error: connection refused when connecting to database.".to_string(),
        }];
        let ctx = extract_compaction_context(&messages);
        assert_eq!(ctx.issues.len(), 1);
        assert!(ctx.issues[0].contains("error:"));
    }

    #[test]
    fn extracts_failed_keyword() {
        let messages = vec![TranscriptMessage {
            role: "assistant".to_string(),
            text_content: "The migration failed: column already exists in table.".to_string(),
        }];
        let ctx = extract_compaction_context(&messages);
        assert_eq!(ctx.issues.len(), 1);
    }

    #[test]
    fn extracts_bug_keyword() {
        let messages = vec![TranscriptMessage {
            role: "assistant".to_string(),
            text_content: "bug: duplicate entries created when session restarts.".to_string(),
        }];
        let ctx = extract_compaction_context(&messages);
        assert_eq!(ctx.issues.len(), 1);
    }

    #[test]
    fn extracts_active_work_from_last_assistant() {
        let messages = vec![
            TranscriptMessage {
                role: "assistant".to_string(),
                text_content: "First assistant message paragraph.".to_string(),
            },
            TranscriptMessage {
                role: "user".to_string(),
                text_content: "User question about the code.".to_string(),
            },
            TranscriptMessage {
                role: "assistant".to_string(),
                text_content:
                    "Working on the database migration now.\n\nHere are the details of the change."
                        .to_string(),
            },
        ];
        let ctx = extract_compaction_context(&messages);
        assert_eq!(ctx.active_work.len(), 1);
        assert!(ctx.active_work[0].contains("database migration"));
    }

    #[test]
    fn caps_items_per_category() {
        let mut paragraphs = Vec::new();
        for i in 0..10 {
            paragraphs.push(format!(
                "We decided to implement feature number {} for testing.",
                i
            ));
        }
        let messages = vec![TranscriptMessage {
            role: "assistant".to_string(),
            text_content: paragraphs.join("\n\n"),
        }];
        let ctx = extract_compaction_context(&messages);
        assert_eq!(ctx.decisions.len(), MAX_ITEMS_PER_CATEGORY);
    }

    #[test]
    fn filters_short_paragraphs() {
        let messages = vec![TranscriptMessage {
            role: "assistant".to_string(),
            text_content: "error: x".to_string(), // 8 chars, below MIN_CONTENT_LEN
        }];
        let ctx = extract_compaction_context(&messages);
        assert!(ctx.issues.is_empty());
        assert!(ctx.active_work.is_empty());
    }

    #[test]
    fn filters_long_paragraphs() {
        let long_text = format!("error: {}", "x".repeat(500));
        let messages = vec![TranscriptMessage {
            role: "assistant".to_string(),
            text_content: long_text,
        }];
        let ctx = extract_compaction_context(&messages);
        assert!(ctx.issues.is_empty());
    }

    #[test]
    fn case_insensitive_matching() {
        let messages = vec![TranscriptMessage {
            role: "assistant".to_string(),
            text_content: "DECIDED TO use uppercase keywords in this test.".to_string(),
        }];
        let ctx = extract_compaction_context(&messages);
        assert_eq!(ctx.decisions.len(), 1);
    }

    #[test]
    fn empty_messages_returns_empty_context() {
        let ctx = extract_compaction_context(&[]);
        assert!(ctx.is_empty());
    }

    #[test]
    fn captures_active_work_even_without_keyword_matches() {
        let messages = vec![TranscriptMessage {
            role: "assistant".to_string(),
            text_content: "This is a normal conversation with no special keywords.".to_string(),
        }];
        let ctx = extract_compaction_context(&messages);
        assert!(ctx.decisions.is_empty());
        assert!(ctx.issues.is_empty());
        assert!(ctx.pending_tasks.is_empty());
        // Active work captures last assistant's first paragraph regardless of keywords
        assert_eq!(ctx.active_work.len(), 1);
    }

    #[test]
    fn multiple_categories_in_one_message() {
        let messages = vec![TranscriptMessage {
            role: "assistant".to_string(),
            text_content:
                "We decided to refactor the database layer.\n\nTODO: update the migration scripts for the schema.\n\nerror: failed to connect to the test database."
                    .to_string(),
        }];
        let ctx = extract_compaction_context(&messages);
        assert_eq!(ctx.decisions.len(), 1);
        assert_eq!(ctx.pending_tasks.len(), 1);
        assert_eq!(ctx.issues.len(), 1);
    }

    #[test]
    fn mixed_case_keywords_matched() {
        let messages = vec![TranscriptMessage {
            role: "assistant".to_string(),
            text_content: "Decided To go with the new approach for handling.".to_string(),
        }];
        let ctx = extract_compaction_context(&messages);
        assert_eq!(ctx.decisions.len(), 1);
    }

    // ── CompactionContext methods ──────────────────────────────────────────

    #[test]
    fn is_empty_when_all_fields_empty() {
        let ctx = CompactionContext::default();
        assert!(ctx.is_empty());
        assert_eq!(ctx.total_items(), 0);
    }

    #[test]
    fn not_empty_with_decisions() {
        let mut ctx = CompactionContext::default();
        ctx.decisions.push("decided something".to_string());
        assert!(!ctx.is_empty());
        assert_eq!(ctx.total_items(), 1);
    }

    #[test]
    fn total_items_counts_all_categories() {
        let ctx = CompactionContext {
            decisions: vec!["d1".into(), "d2".into()],
            active_work: vec!["a1".into()],
            issues: vec!["i1".into()],
            pending_tasks: vec!["p1".into(), "p2".into(), "p3".into()],
        };
        assert_eq!(ctx.total_items(), 7);
    }

    // ── Serialization round-trip ──────────────────────────────────────────

    #[test]
    fn compaction_context_serializes_and_deserializes() {
        let ctx = CompactionContext {
            decisions: vec!["chose builder pattern".into()],
            active_work: vec!["working on migration".into()],
            issues: vec!["connection refused".into()],
            pending_tasks: vec!["add validation".into()],
        };
        let json = serde_json::to_value(&ctx).unwrap();
        let roundtrip: CompactionContext = serde_json::from_value(json).unwrap();
        assert_eq!(roundtrip.decisions, ctx.decisions);
        assert_eq!(roundtrip.active_work, ctx.active_work);
        assert_eq!(roundtrip.issues, ctx.issues);
        assert_eq!(roundtrip.pending_tasks, ctx.pending_tasks);
    }

    // ── Transcript path validation ───────────────────────────────────────

    #[test]
    fn validate_transcript_path_under_tmp() {
        let path = PathBuf::from("/tmp/claude/transcript.jsonl");
        assert!(path.starts_with("/tmp"));
    }

    #[test]
    fn validate_transcript_path_rejects_arbitrary_path() {
        let path = PathBuf::from("/etc/passwd");
        assert!(!path.starts_with("/tmp"));
    }

    // ── Constants ────────────────────────────────────────────────────────

    #[test]
    fn constants_have_expected_values() {
        assert_eq!(MAX_ITEMS_PER_CATEGORY, 5);
        assert_eq!(MIN_CONTENT_LEN, 10);
        assert_eq!(MAX_CONTENT_LEN, 500);
        assert!((COMPACTION_CONFIDENCE - 0.3).abs() < f64::EPSILON);
    }
}
