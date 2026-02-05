// crates/mira-server/src/tools/core/session_notes.rs
// Claude Code session notes integration
//
// Reads session notes from ~/.claude/projects/{path}/{session-id}/session-memory/summary.md
// This feature requires the tengu_session_memory feature flag to be enabled in Claude Code.

use std::path::{Path, PathBuf};

use crate::utils::truncate;

/// Claude Code session note structure
#[derive(Debug, Default)]
pub struct SessionNote {
    pub session_id: String,
    pub title: Option<String>,
    pub current_state: Option<String>,
    pub task_specification: Option<String>,
    pub files_and_functions: Option<String>,
    pub workflow: Option<String>,
    pub errors_and_corrections: Option<String>,
    pub codebase_documentation: Option<String>,
    pub learnings: Option<String>,
    pub key_results: Option<String>,
    pub worklog: Option<String>,
}

/// Get the Claude Code projects directory
fn get_claude_projects_dir() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let claude_dir = home.join(".claude").join("projects");
    if claude_dir.exists() {
        Some(claude_dir)
    } else {
        None
    }
}

/// Sanitize project path to match Claude Code's directory naming
/// /home/peter/Mira -> -home-peter-Mira
fn sanitize_project_path(path: &str) -> String {
    path.replace('/', "-")
}

/// Discover session notes directories for a project
pub fn discover_session_notes(project_path: &str) -> Vec<PathBuf> {
    let Some(claude_projects) = get_claude_projects_dir() else {
        return Vec::new();
    };

    let sanitized = sanitize_project_path(project_path);
    let project_dir = claude_projects.join(&sanitized);

    if !project_dir.exists() {
        return Vec::new();
    }

    let mut notes = Vec::new();

    // Look for session directories with session-memory/summary.md
    if let Ok(entries) = std::fs::read_dir(&project_dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir() {
                let summary_path = path.join("session-memory").join("summary.md");
                if summary_path.exists() {
                    notes.push(summary_path);
                }
            }
        }
    }

    // Sort by modification time (most recent first)
    notes.sort_by(|a, b| {
        let a_time = a.metadata().and_then(|m| m.modified()).ok();
        let b_time = b.metadata().and_then(|m| m.modified()).ok();
        b_time.cmp(&a_time)
    });

    notes
}

/// Parse a session notes markdown file into structured data
pub fn parse_session_note(path: &Path) -> Option<SessionNote> {
    let content = std::fs::read_to_string(path).ok()?;

    // Extract session ID from path: .../session-id/session-memory/summary.md
    let session_id = path
        .parent()? // session-memory
        .parent()? // session-id
        .file_name()?
        .to_str()?
        .to_string();

    let mut note = SessionNote {
        session_id,
        ..Default::default()
    };

    let mut current_section: Option<&mut Option<String>> = None;
    let mut section_content = String::new();

    for line in content.lines() {
        let trimmed = line.trim();

        // Check for section headers
        if trimmed.starts_with("# ") && !trimmed.starts_with("# Session") {
            // Save previous section
            if let Some(section) = current_section.take()
                && !section_content.trim().is_empty() {
                    *section = Some(section_content.trim().to_string());
                }
            section_content.clear();

            // Determine new section
            let header = trimmed.trim_start_matches("# ").to_lowercase();
            current_section = match header.as_str() {
                s if s.contains("title") => {
                    // Title is on the same line
                    note.title = Some(trimmed.trim_start_matches("# ").to_string());
                    None
                }
                s if s.contains("current state") => Some(&mut note.current_state),
                s if s.contains("task specification") => Some(&mut note.task_specification),
                s if s.contains("files") && s.contains("function") => {
                    Some(&mut note.files_and_functions)
                }
                s if s.contains("workflow") => Some(&mut note.workflow),
                s if s.contains("error") && s.contains("correction") => {
                    Some(&mut note.errors_and_corrections)
                }
                s if s.contains("codebase") || s.contains("documentation") => {
                    Some(&mut note.codebase_documentation)
                }
                s if s.contains("learning") => Some(&mut note.learnings),
                s if s.contains("key result") => Some(&mut note.key_results),
                s if s.contains("worklog") => Some(&mut note.worklog),
                _ => None,
            };
        } else if current_section.is_some() {
            // Skip italic description lines (template instructions)
            if trimmed.starts_with('_') && trimmed.ends_with('_') {
                continue;
            }
            section_content.push_str(line);
            section_content.push('\n');
        }
    }

    // Save last section
    if let Some(section) = current_section
        && !section_content.trim().is_empty() {
            *section = Some(section_content.trim().to_string());
        }

    Some(note)
}

/// Get recent session notes for a project
pub fn get_recent_session_notes(project_path: &str, limit: usize) -> Vec<SessionNote> {
    discover_session_notes(project_path)
        .into_iter()
        .take(limit)
        .filter_map(|path| parse_session_note(&path))
        .collect()
}

/// Format session notes for display
pub fn format_session_notes(notes: &[SessionNote]) -> String {
    if notes.is_empty() {
        return String::new();
    }

    let mut output = String::from("\nClaude Code Session Notes:\n");

    for note in notes {
        let title = note.title.as_deref().unwrap_or("(untitled)");
        let short_id = &note.session_id[..note.session_id.len().min(8)];
        output.push_str(&format!("  [{}] {}\n", short_id, title));

        if let Some(state) = &note.current_state {
            let preview = truncate(state, 100);
            output.push_str(&format!("    State: {}\n", preview.replace('\n', " ")));
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_project_path() {
        assert_eq!(
            sanitize_project_path("/home/peter/Mira"),
            "-home-peter-Mira"
        );
        assert_eq!(sanitize_project_path("/tmp/test"), "-tmp-test");
    }

    #[test]
    fn test_parse_session_note() {
        let content = r#"# Implementing Dark Mode Toggle

# Current State
_What is actively being worked on right now?_

Working on the CSS variables for theme switching.
Need to finish the toggle component.

# Task specification
_What did the user ask to build?_

Add dark mode toggle to application settings.

# Files and Functions
_What are the important files?_

- src/components/ThemeToggle.tsx - main toggle component
- src/styles/theme.css - CSS variables

# Learnings
_What has worked well?_

CSS custom properties work well for theming.
"#;

        // Write to temp file (auto-cleaned on drop)
        let temp_dir = tempfile::TempDir::new().unwrap();
        let session_dir = temp_dir.path().join("test-session-id").join("session-memory");
        std::fs::create_dir_all(&session_dir).unwrap();
        let summary_path = session_dir.join("summary.md");
        std::fs::write(&summary_path, content).unwrap();

        let note = parse_session_note(&summary_path).unwrap();

        assert_eq!(note.session_id, "test-session-id");
        assert!(note.current_state.is_some());
        assert!(note.current_state.unwrap().contains("CSS variables"));
        assert!(note.task_specification.is_some());
        assert!(note.files_and_functions.is_some());
        assert!(note.learnings.is_some());
    }
}
