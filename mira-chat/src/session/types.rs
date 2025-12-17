//! Session types for chat persistence

use serde::{Deserialize, Serialize};

use crate::context::MiraContext;

/// A chat message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: String,
    pub role: String,
    pub content: String,
    pub created_at: i64,
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
    /// Previous response ID for OpenAI continuity
    pub previous_response_id: Option<String>,
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
