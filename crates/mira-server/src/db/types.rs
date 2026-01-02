// db/types.rs
// Data structures returned by database operations

// Note: MemoryFact is in mira_types (shared crate)
// Use parse_memory_fact_row() from db/memory.rs for row parsing

/// Tool history entry
#[derive(Debug, Clone)]
pub struct ToolHistoryEntry {
    pub id: i64,
    pub session_id: String,
    pub tool_name: String,
    pub arguments: Option<String>,
    pub result_summary: Option<String>,
    pub success: bool,
    pub created_at: String,
}

/// Session info
#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub id: String,
    pub project_id: Option<i64>,
    pub status: String,
    pub summary: Option<String>,
    pub started_at: String,
    pub last_activity: String,
}

/// Chat message record
#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub id: i64,
    pub role: String,
    pub content: String,
    pub reasoning_content: Option<String>,
    pub created_at: String,
}

/// Chat summary record
#[derive(Debug, Clone)]
pub struct ChatSummary {
    pub id: i64,
    pub summary: String,
    pub message_range_start: i64,
    pub message_range_end: i64,
    pub summary_level: i32,
    pub created_at: String,
}
