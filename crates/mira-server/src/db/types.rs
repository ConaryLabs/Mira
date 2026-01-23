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
    pub project_id: Option<i64>,
    pub summary: String,
    pub message_range_start: i64,
    pub message_range_end: i64,
    pub summary_level: i32,
    pub created_at: String,
}

/// Task record
#[derive(Debug, Clone)]
pub struct Task {
    pub id: i64,
    pub project_id: Option<i64>,
    pub goal_id: Option<i64>,
    pub title: String,
    pub description: Option<String>,
    pub status: String,
    pub priority: String,
    pub created_at: String,
}

/// Goal record
#[derive(Debug, Clone)]
pub struct Goal {
    pub id: i64,
    pub project_id: Option<i64>,
    pub title: String,
    pub description: Option<String>,
    pub status: String,
    pub priority: String,
    pub progress_percent: i32,
    pub created_at: String,
}

/// Project briefing (What's New since last session)
#[derive(Debug, Clone)]
pub struct ProjectBriefing {
    pub project_id: i64,
    pub last_known_commit: Option<String>,
    pub last_session_at: Option<String>,
    pub briefing_text: Option<String>,
    pub generated_at: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================================
    // ToolHistoryEntry tests
    // ============================================================================

    #[test]
    fn test_tool_history_entry_clone() {
        let entry = ToolHistoryEntry {
            id: 1,
            session_id: "session123".to_string(),
            tool_name: "search_code".to_string(),
            arguments: Some(r#"{"query": "test"}"#.to_string()),
            result_summary: Some("Found 5 results".to_string()),
            success: true,
            created_at: "2024-01-01T00:00:00Z".to_string(),
        };
        let cloned = entry.clone();
        assert_eq!(entry.id, cloned.id);
        assert_eq!(entry.session_id, cloned.session_id);
        assert_eq!(entry.tool_name, cloned.tool_name);
        assert_eq!(entry.arguments, cloned.arguments);
        assert_eq!(entry.result_summary, cloned.result_summary);
        assert_eq!(entry.success, cloned.success);
    }

    #[test]
    fn test_tool_history_entry_optional_fields() {
        let entry = ToolHistoryEntry {
            id: 2,
            session_id: "s1".to_string(),
            tool_name: "recall".to_string(),
            arguments: None,
            result_summary: None,
            success: false,
            created_at: "2024-01-01".to_string(),
        };
        assert!(entry.arguments.is_none());
        assert!(entry.result_summary.is_none());
        assert!(!entry.success);
    }

    // ============================================================================
    // SessionInfo tests
    // ============================================================================

    #[test]
    fn test_session_info_clone() {
        let session = SessionInfo {
            id: "session456".to_string(),
            project_id: Some(1),
            status: "active".to_string(),
            summary: Some("Working on tests".to_string()),
            started_at: "2024-01-01T10:00:00Z".to_string(),
            last_activity: "2024-01-01T12:00:00Z".to_string(),
        };
        let cloned = session.clone();
        assert_eq!(session.id, cloned.id);
        assert_eq!(session.project_id, cloned.project_id);
        assert_eq!(session.status, cloned.status);
    }

    #[test]
    fn test_session_info_no_project() {
        let session = SessionInfo {
            id: "orphan".to_string(),
            project_id: None,
            status: "ended".to_string(),
            summary: None,
            started_at: "2024-01-01".to_string(),
            last_activity: "2024-01-01".to_string(),
        };
        assert!(session.project_id.is_none());
        assert!(session.summary.is_none());
    }

    // ============================================================================
    // ChatMessage tests
    // ============================================================================

    #[test]
    fn test_chat_message_clone() {
        let msg = ChatMessage {
            id: 1,
            role: "user".to_string(),
            content: "Hello, world!".to_string(),
            reasoning_content: Some("Thinking...".to_string()),
            created_at: "2024-01-01T00:00:00Z".to_string(),
        };
        let cloned = msg.clone();
        assert_eq!(msg.id, cloned.id);
        assert_eq!(msg.role, cloned.role);
        assert_eq!(msg.content, cloned.content);
        assert_eq!(msg.reasoning_content, cloned.reasoning_content);
    }

    #[test]
    fn test_chat_message_no_reasoning() {
        let msg = ChatMessage {
            id: 2,
            role: "assistant".to_string(),
            content: "Response".to_string(),
            reasoning_content: None,
            created_at: "2024-01-01".to_string(),
        };
        assert!(msg.reasoning_content.is_none());
    }

    // ============================================================================
    // ChatSummary tests
    // ============================================================================

    #[test]
    fn test_chat_summary_clone() {
        let summary = ChatSummary {
            id: 1,
            project_id: Some(10),
            summary: "Discussion about testing".to_string(),
            message_range_start: 1,
            message_range_end: 50,
            summary_level: 1,
            created_at: "2024-01-01".to_string(),
        };
        let cloned = summary.clone();
        assert_eq!(summary.summary, cloned.summary);
        assert_eq!(summary.message_range_start, cloned.message_range_start);
        assert_eq!(summary.message_range_end, cloned.message_range_end);
    }

    // ============================================================================
    // Task tests
    // ============================================================================

    #[test]
    fn test_task_clone() {
        let task = Task {
            id: 1,
            project_id: Some(5),
            goal_id: Some(10),
            title: "Implement feature".to_string(),
            description: Some("Add new functionality".to_string()),
            status: "pending".to_string(),
            priority: "high".to_string(),
            created_at: "2024-01-01".to_string(),
        };
        let cloned = task.clone();
        assert_eq!(task.title, cloned.title);
        assert_eq!(task.status, cloned.status);
        assert_eq!(task.priority, cloned.priority);
    }

    #[test]
    fn test_task_minimal() {
        let task = Task {
            id: 2,
            project_id: None,
            goal_id: None,
            title: "Quick task".to_string(),
            description: None,
            status: "completed".to_string(),
            priority: "low".to_string(),
            created_at: "2024-01-01".to_string(),
        };
        assert!(task.project_id.is_none());
        assert!(task.goal_id.is_none());
        assert!(task.description.is_none());
    }

    // ============================================================================
    // Goal tests
    // ============================================================================

    #[test]
    fn test_goal_clone() {
        let goal = Goal {
            id: 1,
            project_id: Some(3),
            title: "Complete milestone".to_string(),
            description: Some("Finish all tasks".to_string()),
            status: "in_progress".to_string(),
            priority: "critical".to_string(),
            progress_percent: 75,
            created_at: "2024-01-01".to_string(),
        };
        let cloned = goal.clone();
        assert_eq!(goal.title, cloned.title);
        assert_eq!(goal.progress_percent, cloned.progress_percent);
    }

    #[test]
    fn test_goal_progress_bounds() {
        let goal = Goal {
            id: 2,
            project_id: None,
            title: "Test".to_string(),
            description: None,
            status: "planning".to_string(),
            priority: "medium".to_string(),
            progress_percent: 0,
            created_at: "2024-01-01".to_string(),
        };
        assert_eq!(goal.progress_percent, 0);

        let completed = Goal {
            progress_percent: 100,
            ..goal.clone()
        };
        assert_eq!(completed.progress_percent, 100);
    }

    // ============================================================================
    // ProjectBriefing tests
    // ============================================================================

    #[test]
    fn test_project_briefing_clone() {
        let briefing = ProjectBriefing {
            project_id: 1,
            last_known_commit: Some("abc123".to_string()),
            last_session_at: Some("2024-01-01T00:00:00Z".to_string()),
            briefing_text: Some("What's new: Added tests".to_string()),
            generated_at: Some("2024-01-02T00:00:00Z".to_string()),
        };
        let cloned = briefing.clone();
        assert_eq!(briefing.project_id, cloned.project_id);
        assert_eq!(briefing.last_known_commit, cloned.last_known_commit);
        assert_eq!(briefing.briefing_text, cloned.briefing_text);
    }

    #[test]
    fn test_project_briefing_empty() {
        let briefing = ProjectBriefing {
            project_id: 2,
            last_known_commit: None,
            last_session_at: None,
            briefing_text: None,
            generated_at: None,
        };
        assert!(briefing.last_known_commit.is_none());
        assert!(briefing.briefing_text.is_none());
    }
}
