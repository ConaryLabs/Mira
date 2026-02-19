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
const MAX_CONTENT_LEN: usize = 800;

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
    let sid = Some(session_id).filter(|s| !s.is_empty());
    let project_id = client.resolve_project(None, sid).await.map(|(id, _)| id);

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

/// Decision keyword patterns (lowercased).
///
/// These are multi-word phrases to avoid false positives from single-word
/// matches like "picked" or "moving to" that appear in non-decision context.
const DECISION_KEYWORDS: &[&str] = &[
    "decided to",
    "we will use",
    "i chose",
    "let's go with",
    "approach:",
    "we went with",
    "the approach is",
    "opted for",
    "going with",
    "settled on",
    "switched to",
    "using instead",
    "the plan is",
    "strategy:",
    "design decision",
    "trade-off:",
    "tradeoff:",
];

/// Pending task keyword patterns (lowercased).
///
/// Patterns are multi-word to avoid false positives. Removed single-word
/// and conversational patterns ("should also", "after that", "later we",
/// "needs to be") that match normal prose too easily.
const TASK_KEYWORDS: &[&str] = &[
    "todo:",
    "next step",
    "remaining:",
    "still need to",
    "haven't yet",
    "not yet implemented",
    "follow-up:",
    "followup:",
    "left to do",
    "will need to",
    "then we need",
    "blocked on",
    "waiting for",
    "- [ ]",
];

/// Issue keyword patterns (lowercased).
///
/// All patterns are either colon-suffixed or multi-word phrases to avoid
/// false positives from single words that appear in normal discussion.
const ISSUE_KEYWORDS: &[&str] = &[
    "error:",
    "failed:",
    "issue:",
    "bug:",
    "broken:",
    "doesn't work",
    "does not work",
    "a regression",
    "workaround:",
    "fixme:",
    "panicked at",
    "stack trace",
    "compilation error",
    "compile error",
];

/// Check if lowercased text matches any patterns in a keyword list.
fn matches_any(lower: &str, keywords: &[&str]) -> bool {
    keywords.iter().any(|kw| lower.contains(kw))
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
                && matches_any(&lower, DECISION_KEYWORDS)
            {
                ctx.decisions.push(trimmed.to_string());
            }

            if ctx.pending_tasks.len() < MAX_ITEMS_PER_CATEGORY
                && matches_any(&lower, TASK_KEYWORDS)
            {
                ctx.pending_tasks.push(trimmed.to_string());
            }

            if ctx.issues.len() < MAX_ITEMS_PER_CATEGORY && matches_any(&lower, ISSUE_KEYWORDS) {
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
        let long_text = format!("error: {}", "x".repeat(800));
        let messages = vec![TranscriptMessage {
            role: "assistant".to_string(),
            text_content: long_text,
        }];
        let ctx = extract_compaction_context(&messages);
        assert!(ctx.issues.is_empty());
    }

    #[test]
    fn accepts_paragraph_at_max_content_len() {
        // "error: " is 7 chars, so pad to exactly MAX_CONTENT_LEN (800)
        let text = format!("error: {}", "x".repeat(MAX_CONTENT_LEN - 7));
        assert_eq!(text.len(), MAX_CONTENT_LEN);
        let messages = vec![TranscriptMessage {
            role: "assistant".to_string(),
            text_content: text,
        }];
        let ctx = extract_compaction_context(&messages);
        assert_eq!(ctx.issues.len(), 1);
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

    // ── New keyword coverage ──────────────────────────────────────────────

    #[test]
    fn extracts_i_chose_decision() {
        let messages = vec![TranscriptMessage {
            role: "assistant".to_string(),
            text_content: "I chose thiserror over anyhow for the public API surface.".to_string(),
        }];
        let ctx = extract_compaction_context(&messages);
        assert_eq!(ctx.decisions.len(), 1);
    }

    #[test]
    fn extracts_lets_go_with_decision() {
        let messages = vec![TranscriptMessage {
            role: "assistant".to_string(),
            text_content: "Let's go with the builder pattern for config structs.".to_string(),
        }];
        let ctx = extract_compaction_context(&messages);
        assert_eq!(ctx.decisions.len(), 1);
    }

    #[test]
    fn extracts_went_with_decision() {
        let messages = vec![TranscriptMessage {
            role: "assistant".to_string(),
            text_content: "We went with SQLite instead of PostgreSQL for this.".to_string(),
        }];
        let ctx = extract_compaction_context(&messages);
        assert_eq!(ctx.decisions.len(), 1);
    }

    #[test]
    fn extracts_opted_for_decision() {
        let messages = vec![TranscriptMessage {
            role: "assistant".to_string(),
            text_content: "We opted for the async approach to keep things responsive.".to_string(),
        }];
        let ctx = extract_compaction_context(&messages);
        assert_eq!(ctx.decisions.len(), 1);
    }

    #[test]
    fn extracts_settled_on_decision() {
        let messages = vec![TranscriptMessage {
            role: "assistant".to_string(),
            text_content: "After discussion, settled on using thiserror for the crate.".to_string(),
        }];
        let ctx = extract_compaction_context(&messages);
        assert_eq!(ctx.decisions.len(), 1);
    }

    #[test]
    fn extracts_going_with_decision() {
        let messages = vec![TranscriptMessage {
            role: "assistant".to_string(),
            text_content: "Going with the builder pattern for configuration structs.".to_string(),
        }];
        let ctx = extract_compaction_context(&messages);
        assert_eq!(ctx.decisions.len(), 1);
    }

    #[test]
    fn extracts_switched_to_decision() {
        let messages = vec![TranscriptMessage {
            role: "assistant".to_string(),
            text_content: "Switched to using DatabasePool after the connection leak.".to_string(),
        }];
        let ctx = extract_compaction_context(&messages);
        assert_eq!(ctx.decisions.len(), 1);
    }

    #[test]
    fn extracts_blocked_on_task() {
        let messages = vec![TranscriptMessage {
            role: "assistant".to_string(),
            text_content: "We're blocked on the upstream API providing auth tokens.".to_string(),
        }];
        let ctx = extract_compaction_context(&messages);
        assert_eq!(ctx.pending_tasks.len(), 1);
    }

    #[test]
    fn extracts_will_need_to_task() {
        let messages = vec![TranscriptMessage {
            role: "assistant".to_string(),
            text_content: "We will need to update the schema after this migration.".to_string(),
        }];
        let ctx = extract_compaction_context(&messages);
        assert_eq!(ctx.pending_tasks.len(), 1);
    }

    #[test]
    fn extracts_checkbox_task() {
        let messages = vec![TranscriptMessage {
            role: "assistant".to_string(),
            text_content: "- [ ] Add unit tests for the new handler code.".to_string(),
        }];
        let ctx = extract_compaction_context(&messages);
        assert_eq!(ctx.pending_tasks.len(), 1);
    }

    #[test]
    fn extracts_doesnt_work_issue() {
        let messages = vec![TranscriptMessage {
            role: "assistant".to_string(),
            text_content: "The hybrid search doesn't work when embeddings are missing.".to_string(),
        }];
        let ctx = extract_compaction_context(&messages);
        assert_eq!(ctx.issues.len(), 1);
    }

    #[test]
    fn extracts_panicked_at_issue() {
        let messages = vec![TranscriptMessage {
            role: "assistant".to_string(),
            text_content: "Thread panicked at 'index out of bounds' in the parser.".to_string(),
        }];
        let ctx = extract_compaction_context(&messages);
        assert_eq!(ctx.issues.len(), 1);
    }

    #[test]
    fn extracts_regression_issue() {
        let messages = vec![TranscriptMessage {
            role: "assistant".to_string(),
            text_content: "This looks like a regression from the recent refactor work.".to_string(),
        }];
        let ctx = extract_compaction_context(&messages);
        assert_eq!(ctx.issues.len(), 1);
    }

    #[test]
    fn extracts_workaround_issue() {
        let messages = vec![TranscriptMessage {
            role: "assistant".to_string(),
            text_content: "workaround: manually flush the buffer before closing conn.".to_string(),
        }];
        let ctx = extract_compaction_context(&messages);
        assert_eq!(ctx.issues.len(), 1);
    }

    // ── matches_any helper ──────────────────────────────────────────────

    #[test]
    fn matches_any_finds_substring() {
        assert!(matches_any("we decided to use it", DECISION_KEYWORDS));
    }

    #[test]
    fn matches_any_returns_false_on_no_match() {
        assert!(!matches_any(
            "this is a normal sentence",
            DECISION_KEYWORDS
        ));
    }

    #[test]
    fn matches_any_case_sensitive_on_lowered_input() {
        // matches_any expects pre-lowered input
        assert!(matches_any("opted for the new way", DECISION_KEYWORDS));
        assert!(!matches_any("OPTED FOR the new way", DECISION_KEYWORDS));
    }

    // ── Precision: should NOT match (false-positive guards) ─────────────

    #[test]
    fn no_false_positive_on_will_use_behavior() {
        // "will use" was tightened to "we will use" -- bare "will use" matches behavior descriptions
        let messages = vec![TranscriptMessage {
            role: "assistant".to_string(),
            text_content: "This function will use the cached value from the pool.".to_string(),
        }];
        let ctx = extract_compaction_context(&messages);
        assert!(ctx.decisions.is_empty());
    }

    #[test]
    fn no_false_positive_on_choosing() {
        // "choosing" was removed -- matches non-decision prose
        let messages = vec![TranscriptMessage {
            role: "assistant".to_string(),
            text_content: "Choosing a variable name from the suggestions list.".to_string(),
        }];
        let ctx = extract_compaction_context(&messages);
        assert!(ctx.decisions.is_empty());
    }

    #[test]
    fn no_false_positive_on_picked() {
        // "picked" was removed -- too ambiguous
        let messages = vec![TranscriptMessage {
            role: "assistant".to_string(),
            text_content: "I picked up the variable name from the existing code.".to_string(),
        }];
        let ctx = extract_compaction_context(&messages);
        assert!(ctx.decisions.is_empty());
    }

    #[test]
    fn no_false_positive_on_moving_to() {
        // "moving to" was removed -- matches navigation prose
        let messages = vec![TranscriptMessage {
            role: "assistant".to_string(),
            text_content: "Moving to the next file in the directory listing.".to_string(),
        }];
        let ctx = extract_compaction_context(&messages);
        assert!(ctx.decisions.is_empty());
    }

    #[test]
    fn no_false_positive_on_should_also() {
        // "should also" was removed -- matches any suggestion
        let messages = vec![TranscriptMessage {
            role: "assistant".to_string(),
            text_content: "You should also note that this function is pure.".to_string(),
        }];
        let ctx = extract_compaction_context(&messages);
        assert!(ctx.pending_tasks.is_empty());
    }

    #[test]
    fn no_false_positive_on_after_that() {
        // "after that" was removed -- too conversational
        let messages = vec![TranscriptMessage {
            role: "assistant".to_string(),
            text_content: "After that the function returns the computed result.".to_string(),
        }];
        let ctx = extract_compaction_context(&messages);
        assert!(ctx.pending_tasks.is_empty());
    }

    #[test]
    fn no_false_positive_on_unexpected() {
        // "unexpected" was removed -- matches discussion prose
        let messages = vec![TranscriptMessage {
            role: "assistant".to_string(),
            text_content: "This unexpected finding is actually quite interesting.".to_string(),
        }];
        let ctx = extract_compaction_context(&messages);
        assert!(ctx.issues.is_empty());
    }

    #[test]
    fn no_false_positive_on_wrong() {
        // "wrong " was removed -- matches opinions, not errors
        let messages = vec![TranscriptMessage {
            role: "assistant".to_string(),
            text_content: "That's the wrong approach but it still compiles fine.".to_string(),
        }];
        let ctx = extract_compaction_context(&messages);
        assert!(ctx.issues.is_empty());
    }

    #[test]
    fn no_false_positive_on_cannot() {
        // "cannot " was removed -- too conversational
        let messages = vec![TranscriptMessage {
            role: "assistant".to_string(),
            text_content: "We cannot use that pattern here, but there are alternatives."
                .to_string(),
        }];
        let ctx = extract_compaction_context(&messages);
        assert!(ctx.issues.is_empty());
    }

    #[test]
    fn no_false_positive_on_warning_discussion() {
        // "warning:" was removed -- discussing warnings isn't the same as having issues
        let messages = vec![TranscriptMessage {
            role: "assistant".to_string(),
            text_content: "The warning: unused variable lint can be silenced with underscore."
                .to_string(),
        }];
        let ctx = extract_compaction_context(&messages);
        assert!(ctx.issues.is_empty());
    }

    #[test]
    fn no_false_positive_on_regression_tests() {
        // "regression" alone was removed -- "a regression" requires the article
        let messages = vec![TranscriptMessage {
            role: "assistant".to_string(),
            text_content: "We should add regression tests for this module later.".to_string(),
        }];
        let ctx = extract_compaction_context(&messages);
        assert!(ctx.issues.is_empty());
    }

    #[test]
    fn no_false_positive_on_fixme_without_colon() {
        // "fixme" alone was tightened to "fixme:" to avoid matching prose
        let messages = vec![TranscriptMessage {
            role: "assistant".to_string(),
            text_content: "The fixme comments in the codebase should be reviewed.".to_string(),
        }];
        let ctx = extract_compaction_context(&messages);
        assert!(ctx.issues.is_empty());
    }

    #[test]
    fn no_false_positive_normal_prose() {
        // A completely normal paragraph should not match any category
        let messages = vec![TranscriptMessage {
            role: "assistant".to_string(),
            text_content:
                "The function takes a reference and returns an owned string from the input."
                    .to_string(),
        }];
        let ctx = extract_compaction_context(&messages);
        assert!(ctx.decisions.is_empty());
        assert!(ctx.pending_tasks.is_empty());
        assert!(ctx.issues.is_empty());
    }

    // ── Constants ────────────────────────────────────────────────────────

    #[test]
    fn constants_have_expected_values() {
        assert_eq!(MAX_ITEMS_PER_CATEGORY, 5);
        assert_eq!(MIN_CONTENT_LEN, 10);
        assert_eq!(MAX_CONTENT_LEN, 800);
        assert!((COMPACTION_CONFIDENCE - 0.3).abs() < f64::EPSILON);
    }

    // ── Keyword list sanity ─────────────────────────────────────────────

    #[test]
    fn keyword_lists_are_non_empty() {
        assert!(!DECISION_KEYWORDS.is_empty());
        assert!(!TASK_KEYWORDS.is_empty());
        assert!(!ISSUE_KEYWORDS.is_empty());
    }

    #[test]
    fn keyword_lists_are_lowercase() {
        for kw in DECISION_KEYWORDS
            .iter()
            .chain(TASK_KEYWORDS)
            .chain(ISSUE_KEYWORDS)
        {
            assert_eq!(
                *kw,
                kw.to_lowercase(),
                "Keyword '{}' must be lowercase",
                kw
            );
        }
    }
}
