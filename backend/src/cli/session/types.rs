// backend/src/cli/session/types.rs
// Session types for CLI state management

use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::api::ws::session::ChatSession;

/// Represents a CLI session (CLI's view of a backend ChatSession)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliSession {
    /// Session ID (same as backend session ID)
    pub id: String,
    /// Optional human-readable name
    pub name: Option<String>,
    /// Project path this session is associated with
    pub project_path: Option<PathBuf>,
    /// Preview of the last message in the session
    pub last_message: Option<String>,
    /// Number of messages in this session
    pub message_count: u32,
    /// When the session was created
    pub created_at: DateTime<Utc>,
    /// When the session was last active
    pub last_active: DateTime<Utc>,
}

impl CliSession {
    /// Create a new session (for local-only use before backend creation)
    pub fn new(id: String, project_path: Option<PathBuf>) -> Self {
        let now = Utc::now();
        Self {
            id,
            name: None,
            project_path,
            last_message: None,
            message_count: 0,
            created_at: now,
            last_active: now,
        }
    }

    /// Create from a backend ChatSession
    pub fn from_backend(session: ChatSession) -> Self {
        Self {
            id: session.id,
            name: session.name,
            project_path: session.project_path.map(PathBuf::from),
            last_message: session.last_message_preview,
            message_count: session.message_count as u32,
            created_at: Utc.timestamp_opt(session.created_at, 0).unwrap(),
            last_active: Utc.timestamp_opt(session.last_active, 0).unwrap(),
        }
    }

    /// Update the last message preview
    pub fn update_last_message(&mut self, message: &str) {
        // Truncate to first 100 chars
        let preview = if message.len() > 100 {
            format!("{}...", &message[..97])
        } else {
            message.to_string()
        };
        self.last_message = Some(preview);
        self.message_count += 1;
        self.last_active = Utc::now();
    }

    /// Get a display name for the session
    pub fn display_name(&self) -> String {
        if let Some(ref name) = self.name {
            name.clone()
        } else if let Some(ref path) = self.project_path {
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("Unknown")
                .to_string()
        } else {
            format!("Session {}", &self.id[..8])
        }
    }

    /// Get a short preview for display
    pub fn preview(&self) -> String {
        self.last_message
            .clone()
            .unwrap_or_else(|| "(no messages)".to_string())
    }

    /// Format the last active time for display
    pub fn last_active_display(&self) -> String {
        let now = Utc::now();
        let duration = now.signed_duration_since(self.last_active);

        if duration.num_minutes() < 1 {
            "just now".to_string()
        } else if duration.num_minutes() < 60 {
            format!("{}m ago", duration.num_minutes())
        } else if duration.num_hours() < 24 {
            format!("{}h ago", duration.num_hours())
        } else if duration.num_days() < 7 {
            format!("{}d ago", duration.num_days())
        } else {
            self.last_active.format("%Y-%m-%d").to_string()
        }
    }
}

/// Session filter options for listing
#[derive(Debug, Clone, Default)]
pub struct SessionFilter {
    /// Filter by project path
    pub project_path: Option<PathBuf>,
    /// Maximum number of sessions to return
    pub limit: Option<usize>,
    /// Search term to filter by name or message content
    pub search: Option<String>,
}

impl SessionFilter {
    /// Create a new filter
    pub fn new() -> Self {
        Self::default()
    }

    /// Set project path filter
    pub fn with_project(mut self, path: PathBuf) -> Self {
        self.project_path = Some(path);
        self
    }

    /// Set limit
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Set search term
    pub fn with_search(mut self, search: String) -> Self {
        self.search = Some(search);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_session() {
        let session = CliSession::new("session-123".to_string(), None);
        assert_eq!(session.id, "session-123");
        assert_eq!(session.message_count, 0);
        assert!(session.last_message.is_none());
    }

    #[test]
    fn test_from_backend() {
        let backend_session = ChatSession {
            id: "backend-123".to_string(),
            user_id: None,
            name: Some("Test Session".to_string()),
            project_path: Some("/home/user/project".to_string()),
            branch: Some("main".to_string()),
            last_message_preview: Some("Hello".to_string()),
            message_count: 5,
            status: "active".to_string(),
            last_commit_hash: None,
            created_at: 1700000000,
            last_active: 1700001000,
        };
        let cli_session = CliSession::from_backend(backend_session);
        assert_eq!(cli_session.id, "backend-123");
        assert_eq!(cli_session.name, Some("Test Session".to_string()));
        assert_eq!(cli_session.message_count, 5);
    }

    #[test]
    fn test_update_last_message() {
        let mut session = CliSession::new("session-123".to_string(), None);
        session.update_last_message("Hello, world!");
        assert_eq!(session.message_count, 1);
        assert_eq!(session.last_message, Some("Hello, world!".to_string()));
    }

    #[test]
    fn test_truncate_long_message() {
        let mut session = CliSession::new("session-123".to_string(), None);
        let long_message = "a".repeat(200);
        session.update_last_message(&long_message);
        assert!(session.last_message.as_ref().unwrap().len() <= 100);
        assert!(session.last_message.as_ref().unwrap().ends_with("..."));
    }

    #[test]
    fn test_display_name() {
        let mut session = CliSession::new("session-123".to_string(), None);
        assert!(session.display_name().starts_with("Session "));

        session.name = Some("My Session".to_string());
        assert_eq!(session.display_name(), "My Session");

        session.name = None;
        session.project_path = Some(PathBuf::from("/home/user/my-project"));
        assert_eq!(session.display_name(), "my-project");
    }
}
