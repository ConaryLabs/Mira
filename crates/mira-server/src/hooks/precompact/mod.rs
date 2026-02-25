// crates/mira-server/src/hooks/precompact/mod.rs
// PreCompact hook handler - preserves context before summarization

mod extract;
#[cfg(test)]
mod tests;

use crate::ipc::client::HookClient;
use crate::utils::truncate_at_boundary;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

#[cfg(test)]
pub(crate) use extract::{extract_and_save_context, extract_compaction_context};

/// Confidence level for compaction log entries
const COMPACTION_CONFIDENCE: f64 = 0.3;
/// Maximum items per category in compaction context
pub(super) const MAX_ITEMS_PER_CATEGORY: usize = 5;
/// Minimum content length for extracted paragraphs (skip trivial entries)
pub(super) const MIN_CONTENT_LEN: usize = 10;
/// Maximum content length for extracted paragraphs (truncate beyond this)
pub(super) const MAX_CONTENT_LEN: usize = 800;
/// Maximum transcript file size (50 MB) -- skip reading to prevent OOM
const MAX_TRANSCRIPT_BYTES: u64 = 50 * 1024 * 1024;
/// Maximum file path references to extract
pub(super) const MAX_FILE_REFS: usize = 10;
/// Minimum match length for file path regex (filters out noise)
pub(super) const MIN_FILE_PATH_LEN: usize = 5;

/// A parsed message from the JSONL transcript
#[derive(Debug)]
pub(crate) struct TranscriptMessage {
    pub(super) role: String,
    pub(super) text_content: String,
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
    #[serde(default)]
    pub user_intent: Option<String>,
    #[serde(default)]
    pub files_referenced: Vec<String>,
}

impl CompactionContext {
    pub(super) fn is_empty(&self) -> bool {
        self.decisions.is_empty()
            && self.active_work.is_empty()
            && self.issues.is_empty()
            && self.pending_tasks.is_empty()
            && self.user_intent.is_none()
            && self.files_referenced.is_empty()
    }

    pub(super) fn total_items(&self) -> usize {
        self.decisions.len()
            + self.active_work.len()
            + self.issues.len()
            + self.pending_tasks.len()
            + self.user_intent.as_ref().map_or(0, |_| 1)
            + self.files_referenced.len()
    }
}

/// Merge a new compaction context into an existing one.
///
/// Vec fields: combine old + new, deduplicate (exact string match), keep the
/// last `MAX_ITEMS_PER_CATEGORY` (or `MAX_FILE_REFS` for files) entries so
/// that recent items are preferred.
///
/// `user_intent`: keep the FIRST one (the original intent from the earliest
/// compaction). Only set if the existing value is `None`.
pub(crate) fn merge_compaction_contexts(
    existing: &serde_json::Value,
    new: &serde_json::Value,
) -> serde_json::Value {
    let old: CompactionContext = match serde_json::from_value(existing.clone()) {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!("Failed to deserialize existing compaction context: {e}");
            CompactionContext::default()
        }
    };
    let incoming: CompactionContext = match serde_json::from_value(new.clone()) {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!("Failed to deserialize incoming compaction context: {e}");
            CompactionContext::default()
        }
    };

    let merged = CompactionContext {
        decisions: merge_vec_field(&old.decisions, &incoming.decisions, MAX_ITEMS_PER_CATEGORY),
        active_work: merge_vec_field(
            &old.active_work,
            &incoming.active_work,
            MAX_ITEMS_PER_CATEGORY,
        ),
        issues: merge_vec_field(&old.issues, &incoming.issues, MAX_ITEMS_PER_CATEGORY),
        pending_tasks: merge_vec_field(
            &old.pending_tasks,
            &incoming.pending_tasks,
            MAX_ITEMS_PER_CATEGORY,
        ),
        user_intent: old.user_intent.or(incoming.user_intent),
        files_referenced: merge_vec_field(
            &old.files_referenced,
            &incoming.files_referenced,
            MAX_FILE_REFS,
        ),
    };

    serde_json::to_value(&merged).unwrap_or_else(|_| new.clone())
}

/// Combine two Vec<String> fields: append new after old, deduplicate by exact
/// match (keeping the later occurrence), then keep only the last `max` items.
fn merge_vec_field(old: &[String], new: &[String], max: usize) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut combined: Vec<String> = Vec::with_capacity(old.len() + new.len());

    // Walk in reverse so when we reverse back, the *last* occurrence wins
    for item in old.iter().chain(new.iter()).rev() {
        if seen.insert(item.as_str().to_owned()) {
            combined.push(item.clone());
        }
    }
    combined.reverse();

    // Keep the last `max` entries (prefer recent)
    if combined.len() > max {
        combined.drain(..combined.len() - max);
    }
    combined
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

    // Read transcript if available (with size guard to prevent OOM)
    let transcript = transcript_path.as_ref().and_then(|p| {
        match fs::metadata(p) {
            Ok(meta) if meta.len() > MAX_TRANSCRIPT_BYTES => {
                tracing::warn!(
                    path = %p.display(),
                    size_mb = meta.len() / (1024 * 1024),
                    "Skipping transcript read: file exceeds 50 MB limit"
                );
                return None;
            }
            Err(e) => {
                tracing::debug!(error = %e, "Could not stat transcript file");
                return None;
            }
            _ => {}
        }
        fs::read_to_string(p)
            .map_err(|e| {
                tracing::debug!(error = %e, path = %p.display(), "Failed to read transcript file");
                e
            })
            .ok()
    });

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
        && let Err(e) = extract::extract_and_save_context(client, session_id, transcript).await
    {
        tracing::warn!(error = %e, "Context extraction failed");
    }

    // Set a post-compaction flag so the next UserPromptSubmit can inject
    // a recovery summary (decisions, active work, files modified, etc.)
    set_post_compaction_flag(session_id);

    tracing::debug!("Pre-compaction state saved");
    Ok(())
}

// ── Post-compaction flag ─────────────────────────────────────────────────
// Written by PreCompact, consumed by UserPromptSubmit on the next prompt.

/// Path to the post-compaction flag file for a session
pub(crate) fn post_compaction_flag_path(session_id: &str) -> PathBuf {
    let mira_dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".mira")
        .join("tmp");
    let sanitized: String = session_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    let sid = if sanitized.len() > 16 {
        sanitized[..16].to_string()
    } else {
        sanitized
    };
    mira_dir.join(format!("post_compact_{}.flag", sid))
}

/// Set the post-compaction flag for a session
pub(super) fn set_post_compaction_flag(session_id: &str) {
    if session_id.is_empty() || session_id == "unknown" {
        return;
    }
    let path = post_compaction_flag_path(session_id);
    if let Some(parent) = path.parent()
        && let Err(e) = fs::create_dir_all(parent)
    {
        tracing::warn!("post-compaction flag: failed to create dir: {e}");
        return;
    }
    // Write a timestamp so we can age-out stale flags
    if let Err(e) = fs::write(&path, format!("{}", crate::hooks::pre_tool::unix_now())) {
        tracing::warn!("post-compaction flag: failed to write flag file: {e}");
    }
}

/// Check whether a post-compaction flag exists and is fresh, without consuming it.
pub(crate) fn check_post_compaction_flag(session_id: &str) -> bool {
    if session_id.is_empty() {
        return false;
    }
    let path = post_compaction_flag_path(session_id);
    if !path.is_file() {
        return false;
    }
    fs::read_to_string(&path)
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map(|ts| crate::hooks::pre_tool::unix_now().saturating_sub(ts) < 600)
        .unwrap_or(false)
}

/// Check and consume the post-compaction flag.
/// Returns true if compaction just happened (flag was present and fresh).
pub(crate) fn consume_post_compaction_flag(session_id: &str) -> bool {
    if session_id.is_empty() {
        return false;
    }
    let path = post_compaction_flag_path(session_id);
    if !path.is_file() {
        return false;
    }
    // Read timestamp and check if flag is fresh (< 10 minutes)
    let is_fresh = fs::read_to_string(&path)
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map(|ts| crate::hooks::pre_tool::unix_now().saturating_sub(ts) < 600)
        .unwrap_or(false);

    // Always remove the flag (consume it)
    let _ = fs::remove_file(&path);
    is_fresh
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
