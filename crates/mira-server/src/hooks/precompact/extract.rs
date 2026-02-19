// crates/mira-server/src/hooks/precompact/extract.rs
// Keyword matching and structured context extraction from transcripts.

use super::{CompactionContext, TranscriptMessage, MAX_CONTENT_LEN, MAX_FILE_REFS, MIN_CONTENT_LEN, MIN_FILE_PATH_LEN, MAX_ITEMS_PER_CATEGORY};
use crate::ipc::client::HookClient;
use crate::utils::truncate_at_boundary;
use anyhow::Result;
use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

// ═══════════════════════════════════════════════════════════════════════
// Keyword Lists
// ═══════════════════════════════════════════════════════════════════════

/// Decision keyword patterns (lowercased).
///
/// These are multi-word phrases to avoid false positives from single-word
/// matches like "picked" or "moving to" that appear in non-decision context.
pub(super) const DECISION_KEYWORDS: &[&str] = &[
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
pub(super) const TASK_KEYWORDS: &[&str] = &[
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
pub(super) const ISSUE_KEYWORDS: &[&str] = &[
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

/// Continuation prompt patterns (lowercased).
/// These are generic session-continuation phrases that carry no meaningful intent.
pub(super) const CONTINUATION_PATTERNS: &[&str] = &[
    "continue",
    "keep going",
    "carry on",
    "go on",
    "go ahead",
    "resume",
    "yes",
    "yeah",
    "yep",
    "ok",
    "okay",
    "sure",
    "do it",
    "let's do it",
    "sounds good",
    "lgtm",
    "proceed",
];

// ═══════════════════════════════════════════════════════════════════════
// Matching Helpers
// ═══════════════════════════════════════════════════════════════════════

/// Check if lowercased text matches any patterns in a keyword list.
pub(super) fn matches_any(lower: &str, keywords: &[&str]) -> bool {
    keywords.iter().any(|kw| lower.contains(kw))
}

/// Check issue keywords only within the first ~80 chars of the paragraph.
/// Real error reports lead with the error pattern; matching the full text
/// produces false positives from incidental mentions.
pub(super) fn matches_issue_keyword(lower: &str) -> bool {
    let prefix = if lower.len() > 80 {
        &lower[..lower.floor_char_boundary(80)]
    } else {
        lower
    };
    ISSUE_KEYWORDS.iter().any(|kw| prefix.contains(kw))
}

/// Check if the user's first message is a generic continuation prompt
/// rather than a meaningful request.
pub(super) fn is_continuation_prompt(text: &str) -> bool {
    let lower = text.to_lowercase();
    let trimmed = lower.trim_end_matches(|c: char| c.is_ascii_punctuation() || c.is_whitespace());
    CONTINUATION_PATTERNS.contains(&trimmed)
}

// ═══════════════════════════════════════════════════════════════════════
// File Path Extraction
// ═══════════════════════════════════════════════════════════════════════

/// Regex for common source file paths referenced in assistant messages.
static FILE_PATH_RE: LazyLock<Regex> = LazyLock::new(|| {
    // SAFETY: This is a static literal regex pattern; compilation cannot fail.
    #[allow(clippy::expect_used)]
    Regex::new(
        r"[\w/.@-]+\.(rs|py|ts|js|tsx|jsx|toml|json|md|yaml|yml|go|java|rb|sh|css|html|sql)\b",
    )
    .expect("file path regex")
});

/// Extract file paths referenced in assistant messages.
pub(super) fn extract_file_paths(messages: &[TranscriptMessage]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut paths = Vec::new();
    for msg in messages.iter().rev() {
        if msg.role != "assistant" {
            continue;
        }
        for m in FILE_PATH_RE.find_iter(&msg.text_content) {
            let path = m.as_str();
            // Skip very short matches and URL-like fragments
            // The regex can't match ":" so URLs like https://docs.rs/foo.html
            // get captured as "//docs.rs/foo.html". Filter those too.
            if path.len() < MIN_FILE_PATH_LEN || path.contains("://") || path.starts_with("//") {
                continue;
            }
            if seen.insert(path.to_string()) {
                paths.push(path.to_string());
                if paths.len() >= MAX_FILE_REFS {
                    return paths;
                }
            }
        }
    }
    paths
}

// ═══════════════════════════════════════════════════════════════════════
// Context Extraction
// ═══════════════════════════════════════════════════════════════════════

/// Extract structured context from parsed transcript messages.
///
/// Iterates messages in reverse so the 5-item cap captures the most recent
/// matches. After collection, reverses each vec to restore chronological order.
/// Also extracts user intent and referenced file paths.
pub(crate) fn extract_compaction_context(messages: &[TranscriptMessage]) -> CompactionContext {
    let mut ctx = CompactionContext::default();

    // Extract user_intent from the first user message that isn't a
    // continuation prompt ("keep going", "continue", etc.)
    for user_msg in messages.iter().filter(|m| m.role == "user") {
        let Some(first_para) = user_msg
            .text_content
            .split("\n\n")
            .next()
            .map(|s| s.trim())
            .filter(|s| s.len() >= MIN_CONTENT_LEN)
        else {
            continue;
        };
        // Skip generic continuation prompts that carry no intent
        if is_continuation_prompt(first_para) {
            continue;
        }
        let intent = if first_para.len() > MAX_CONTENT_LEN {
            truncate_at_boundary(first_para, MAX_CONTENT_LEN).to_string()
        } else {
            first_para.to_string()
        };
        ctx.user_intent = Some(intent);
        break;
    }

    // Extract file paths from assistant messages
    ctx.files_referenced = extract_file_paths(messages);

    // Reverse iteration: scan from most recent to oldest so the 5-item cap
    // captures the most recent matches. Only scan assistant messages to avoid
    // capturing user descriptions ("I decided to...") as actual decisions.
    for msg in messages.iter().rev() {
        if msg.role != "assistant" {
            continue;
        }
        for paragraph in msg.text_content.split("\n\n") {
            let trimmed = paragraph.trim();
            if trimmed.len() < MIN_CONTENT_LEN {
                continue;
            }
            // Truncate instead of dropping paragraphs that exceed MAX_CONTENT_LEN
            let content = if trimmed.len() > MAX_CONTENT_LEN {
                truncate_at_boundary(trimmed, MAX_CONTENT_LEN)
            } else {
                trimmed
            };
            let lower = content.to_lowercase();

            if ctx.decisions.len() < MAX_ITEMS_PER_CATEGORY
                && matches_any(&lower, DECISION_KEYWORDS)
            {
                ctx.decisions.push(content.to_string());
            }

            if ctx.pending_tasks.len() < MAX_ITEMS_PER_CATEGORY
                && matches_any(&lower, TASK_KEYWORDS)
            {
                ctx.pending_tasks.push(content.to_string());
            }

            if ctx.issues.len() < MAX_ITEMS_PER_CATEGORY && matches_issue_keyword(&lower) {
                ctx.issues.push(content.to_string());
            }
        }
    }

    // Restore chronological order after reverse collection
    ctx.decisions.reverse();
    ctx.pending_tasks.reverse();
    ctx.issues.reverse();

    // Capture active work: walk backward to find the last assistant message
    // with substantial text, take up to 2 paragraphs.
    for msg in messages.iter().rev() {
        if msg.role != "assistant" {
            continue;
        }
        let paras: Vec<&str> = msg
            .text_content
            .split("\n\n")
            .map(|s| s.trim())
            .filter(|s| s.len() > 30)
            .take(2)
            .collect();
        if !paras.is_empty() {
            for p in paras {
                let content = if p.len() > MAX_CONTENT_LEN {
                    truncate_at_boundary(p, MAX_CONTENT_LEN)
                } else {
                    p
                };
                ctx.active_work.push(content.to_string());
            }
            break;
        }
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
    let messages = super::parse_transcript_messages(transcript);
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
