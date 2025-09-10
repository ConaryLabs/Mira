//! Handles LLM-driven summarization of memories for long-term retention/context condensation.

use crate::memory::core::types::MemoryEntry;

/// Summarize a batch of memory entries (calls LLM outside this function).
pub async fn summarize_memories(entries: &[MemoryEntry]) -> Option<String> {
    // Placeholder: You’ll wire in the actual LLM summarization call here.
    // For now, just join the first sentences as a stub.
    let summary = entries
        .iter()
        .filter_map(|e| e.content.split('.').next())
        .collect::<Vec<_>>()
        .join(" / ");
    if summary.is_empty() {
        None
    } else {
        Some(summary)
    }
}
