// crates/mira-server/src/tools/core/session_notes.rs
// Claude Code session notes integration
//
// Reads session notes from ~/.claude/projects/{path}/{session-id}/session-memory/summary.md
// This feature requires the tengu_session_memory feature flag to be enabled in Claude Code.

use std::path::{Path, PathBuf};

use crate::utils::{sanitize_project_path, truncate};

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

// sanitize_project_path is imported from crate::utils

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
                && !section_content.trim().is_empty()
            {
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
        && !section_content.trim().is_empty()
    {
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

    // Helper: create a session note file at the expected path structure
    // Returns (TempDir, PathBuf) where PathBuf is the summary.md path
    fn write_note(session_id: &str, content: &str) -> (tempfile::TempDir, PathBuf) {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let session_dir = temp_dir.path().join(session_id).join("session-memory");
        std::fs::create_dir_all(&session_dir).unwrap();
        let summary_path = session_dir.join("summary.md");
        std::fs::write(&summary_path, content).unwrap();
        (temp_dir, summary_path)
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // sanitize_project_path
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_sanitize_project_path() {
        assert_eq!(
            sanitize_project_path("/home/peter/Mira"),
            "-home-peter-Mira"
        );
        assert_eq!(sanitize_project_path("/tmp/test"), "-tmp-test");
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // parse_session_note
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_parse_all_sections() {
        let content = r#"# Title: Dark Mode Toggle

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

# Workflow
_Steps taken._

1. Created component
2. Added styles

# Errors and Corrections
_What went wrong?_

Initially used rgba, switched to hsl.

# Codebase Documentation
_What do we know?_

Uses React 18 with CSS modules.

# Learnings
_What has worked well?_

CSS custom properties work well for theming.

# Key Results
_What was accomplished?_

Dark mode toggle working in dev.

# Worklog
_Timeline._

10:00 - Started work
11:30 - Finished toggle
"#;

        let (_dir, path) = write_note("test-session-id", content);
        let note = parse_session_note(&path).unwrap();

        assert_eq!(note.session_id, "test-session-id");
        // Title header must contain "title" to be parsed as title
        assert_eq!(note.title.as_deref(), Some("Title: Dark Mode Toggle"));
        assert!(
            note.current_state
                .as_ref()
                .unwrap()
                .contains("CSS variables")
        );
        assert!(
            note.task_specification
                .as_ref()
                .unwrap()
                .contains("dark mode")
        );
        assert!(
            note.files_and_functions
                .as_ref()
                .unwrap()
                .contains("ThemeToggle.tsx")
        );
        assert!(
            note.workflow
                .as_ref()
                .unwrap()
                .contains("Created component")
        );
        assert!(
            note.errors_and_corrections
                .as_ref()
                .unwrap()
                .contains("rgba")
        );
        assert!(
            note.codebase_documentation
                .as_ref()
                .unwrap()
                .contains("React 18")
        );
        assert!(
            note.learnings
                .as_ref()
                .unwrap()
                .contains("CSS custom properties")
        );
        assert!(
            note.key_results
                .as_ref()
                .unwrap()
                .contains("Dark mode toggle")
        );
        assert!(note.worklog.as_ref().unwrap().contains("10:00"));
    }

    #[test]
    fn test_parse_non_title_heading_ignored() {
        // Headings that don't contain "title" are not captured as title
        let content = "# Implementing Dark Mode Toggle\n\n# Current State\nWorking.\n";
        let (_dir, path) = write_note("no-title", content);
        let note = parse_session_note(&path).unwrap();
        assert!(note.title.is_none());
        assert!(note.current_state.as_ref().unwrap().contains("Working"));
    }

    #[test]
    fn test_parse_empty_file() {
        let (_dir, path) = write_note("empty-session", "");
        let note = parse_session_note(&path).unwrap();

        assert_eq!(note.session_id, "empty-session");
        assert!(note.title.is_none());
        assert!(note.current_state.is_none());
        assert!(note.task_specification.is_none());
    }

    #[test]
    fn test_parse_title_only() {
        let (_dir, path) = write_note("title-only", "# My Project Title\n");
        let note = parse_session_note(&path).unwrap();

        assert_eq!(note.title.as_deref(), Some("My Project Title"));
        assert!(note.current_state.is_none());
    }

    #[test]
    fn test_parse_sections_with_empty_content() {
        let content = "# Title\n\n# Current State\n\n# Learnings\n\nActual content here.\n";
        let (_dir, path) = write_note("sparse", content);
        let note = parse_session_note(&path).unwrap();

        // Current State has no content (only empty lines)
        assert!(note.current_state.is_none());
        // Learnings has content
        assert_eq!(note.learnings.as_deref(), Some("Actual content here."));
    }

    #[test]
    fn test_parse_filters_italic_description_lines() {
        let content = "# Current State\n_This is a template instruction_\nReal content here.\n";
        let (_dir, path) = write_note("filter-test", content);
        let note = parse_session_note(&path).unwrap();

        let state = note.current_state.unwrap();
        assert!(!state.contains("template instruction"));
        assert!(state.contains("Real content here."));
    }

    #[test]
    fn test_parse_session_header_skipped() {
        // "# Session ..." headers should be skipped per the condition
        let content = "# Session Summary\n\n# Current State\nWorking on tests.\n";
        let (_dir, path) = write_note("session-header", content);
        let note = parse_session_note(&path).unwrap();

        // Title should not be set to "Session Summary" since it starts with "# Session"
        assert!(note.title.is_none());
        assert!(
            note.current_state
                .as_ref()
                .unwrap()
                .contains("Working on tests")
        );
    }

    #[test]
    fn test_parse_unknown_section_ignored() {
        let content = "# Random Unknown Section\nThis content should be ignored.\n\n# Current State\nActual state.\n";
        let (_dir, path) = write_note("unknown-section", content);
        let note = parse_session_note(&path).unwrap();

        assert!(
            note.current_state
                .as_ref()
                .unwrap()
                .contains("Actual state")
        );
    }

    #[test]
    fn test_parse_multiline_section_content() {
        let content = "# Current State\nLine 1\nLine 2\nLine 3\n";
        let (_dir, path) = write_note("multiline", content);
        let note = parse_session_note(&path).unwrap();

        let state = note.current_state.unwrap();
        assert!(state.contains("Line 1"));
        assert!(state.contains("Line 2"));
        assert!(state.contains("Line 3"));
    }

    #[test]
    fn test_parse_nonexistent_file() {
        let result = parse_session_note(Path::new("/tmp/nonexistent/summary.md"));
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_session_id_from_path() {
        let (_dir, path) = write_note("abc-def-123-456", "# Title\n");
        let note = parse_session_note(&path).unwrap();
        assert_eq!(note.session_id, "abc-def-123-456");
    }

    #[test]
    fn test_parse_documentation_section_alias() {
        // "documentation" alone (without "codebase") should also match
        let content = "# Documentation\nSome docs here.\n";
        let (_dir, path) = write_note("doc-alias", content);
        let note = parse_session_note(&path).unwrap();
        assert!(
            note.codebase_documentation
                .as_ref()
                .unwrap()
                .contains("Some docs here")
        );
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // format_session_notes
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_format_empty_notes() {
        let result = format_session_notes(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_format_single_note_with_title() {
        let note = SessionNote {
            session_id: "abcdef12-3456-7890".to_string(),
            title: Some("Working on tests".to_string()),
            current_state: Some("Writing unit tests".to_string()),
            ..Default::default()
        };
        let result = format_session_notes(&[note]);

        assert!(result.contains("Claude Code Session Notes:"));
        assert!(result.contains("[abcdef12]")); // truncated to 8 chars
        assert!(result.contains("Working on tests"));
        assert!(result.contains("State: Writing unit tests"));
    }

    #[test]
    fn test_format_note_without_title() {
        let note = SessionNote {
            session_id: "12345678".to_string(),
            ..Default::default()
        };
        let result = format_session_notes(&[note]);
        assert!(result.contains("(untitled)"));
    }

    #[test]
    fn test_format_note_without_state() {
        let note = SessionNote {
            session_id: "12345678".to_string(),
            title: Some("No state note".to_string()),
            current_state: None,
            ..Default::default()
        };
        let result = format_session_notes(&[note]);
        assert!(result.contains("No state note"));
        assert!(!result.contains("State:"));
    }

    #[test]
    fn test_format_note_state_newlines_replaced() {
        let note = SessionNote {
            session_id: "12345678".to_string(),
            title: Some("Test".to_string()),
            current_state: Some("Line 1\nLine 2\nLine 3".to_string()),
            ..Default::default()
        };
        let result = format_session_notes(&[note]);
        // Newlines should be replaced with spaces
        assert!(result.contains("Line 1 Line 2 Line 3"));
    }

    #[test]
    fn test_format_multiple_notes() {
        let notes = vec![
            SessionNote {
                session_id: "session-aaa".to_string(),
                title: Some("First".to_string()),
                ..Default::default()
            },
            SessionNote {
                session_id: "session-bbb".to_string(),
                title: Some("Second".to_string()),
                ..Default::default()
            },
        ];
        let result = format_session_notes(&notes);
        assert!(result.contains("First"));
        assert!(result.contains("Second"));
    }

    #[test]
    fn test_format_short_session_id() {
        let note = SessionNote {
            session_id: "ab".to_string(), // shorter than 8
            title: Some("Short".to_string()),
            ..Default::default()
        };
        let result = format_session_notes(&[note]);
        assert!(result.contains("[ab]")); // min(2, 8) = 2
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // discover_session_notes + get_recent_session_notes
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_discover_nonexistent_project() {
        // A path that definitely won't exist in ~/.claude/projects/
        let result = discover_session_notes("/nonexistent/path/zzz_fake_project_999");
        assert!(result.is_empty());
    }

    #[test]
    fn test_get_recent_session_notes_nonexistent() {
        let result = get_recent_session_notes("/nonexistent/path/zzz_fake_project_999", 5);
        assert!(result.is_empty());
    }
}
