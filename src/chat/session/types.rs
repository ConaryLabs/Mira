//! Session types for chat persistence

use serde::{Deserialize, Serialize};

use crate::chat::context::MiraContext;
use super::git_tracker::RepoActivity;

/// A chat message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: String,
    pub role: String,
    pub content: String,
    pub created_at: i64,
}

/// A small hint about a symbol from the background code index.
#[derive(Debug, Clone)]
pub struct CodeIndexSymbolHint {
    pub name: String,
    pub qualified_name: Option<String>,
    pub symbol_type: String,
    pub signature: Option<String>,
    pub start_line: i64,
    pub end_line: i64,
}

/// Code-index hints for a specific file.
#[derive(Debug, Clone)]
pub struct CodeIndexFileHint {
    pub file_path: String,
    pub symbols: Vec<CodeIndexSymbolHint>,
}

/// A rejected approach that didn't work
#[derive(Debug, Clone, Default)]
pub struct RejectedApproach {
    pub problem_context: String,
    pub approach: String,
    pub rejection_reason: String,
}

/// A past decision with context
#[derive(Debug, Clone, Default)]
pub struct PastDecision {
    pub key: String,
    pub decision: String,
    pub context: Option<String>,
}

/// Assembled context for a query
#[derive(Debug, Default)]
pub struct AssembledContext {
    /// Recent messages in the sliding window
    pub recent_messages: Vec<ChatMessage>,
    /// Semantically relevant past context
    pub semantic_context: Vec<SemanticHit>,
    /// Mira context (corrections, goals, memories)
    pub mira_context: MiraContext,
    /// Rolling summaries of older conversation
    pub summaries: Vec<String>,
    /// Code compaction blob (if available)
    pub code_compaction: Option<String>,
    /// Query-dependent hints from the background code index (symbols/files)
    pub code_index_hints: Vec<CodeIndexFileHint>,
    /// Previous response ID for OpenAI continuity
    pub previous_response_id: Option<String>,
    /// Recent git activity (commits, changed files)
    pub repo_activity: Option<RepoActivity>,
    /// Rejected approaches - things that didn't work (anti-amnesia)
    pub rejected_approaches: Vec<RejectedApproach>,
    /// Past decisions with context (anti-amnesia)
    pub past_decisions: Vec<PastDecision>,
}

/// A semantic search hit
#[derive(Debug, Clone)]
pub struct SemanticHit {
    pub content: String,
    pub score: f32,
    pub role: String,
    pub created_at: i64,
}

/// Session statistics
pub struct SessionStats {
    pub total_messages: usize,
    pub summary_count: usize,
    pub has_active_conversation: bool,
    pub has_code_compaction: bool,
}

/// A checkpoint for DeepSeek continuity
///
/// Created after each successful unit of progress (tool success, test pass, etc.)
/// Provides compact state summary to replace server-side chain continuity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    /// Unique checkpoint ID
    pub id: String,
    /// What task/goal we're working on
    pub current_task: String,
    /// What just changed (last action summary)
    pub last_action: String,
    /// What's remaining to do
    pub remaining: Option<String>,
    /// Files being actively worked on
    pub working_files: Vec<String>,
    /// Relevant artifact IDs for context
    pub artifact_ids: Vec<String>,
    /// When this checkpoint was created
    pub created_at: i64,
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chat_message_serialize() {
        let msg = ChatMessage {
            id: "test".into(),
            role: "user".into(),
            content: "Hello".into(),
            created_at: 12345,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("Hello"));
    }
}
