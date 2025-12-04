// backend/src/cli/project/context.rs
// Build message metadata from detected project context

use crate::api::ws::message::MessageMetadata;

use super::detector::DetectedProject;

/// Build MessageMetadata for a chat message from project context
pub fn build_metadata(project: &DetectedProject) -> MessageMetadata {
    MessageMetadata {
        file_path: None,
        file_content: None,
        repo_id: None,
        attachment_id: None,
        language: None,
        selection: None,
        project_name: Some(project.name.clone()),
        has_repository: Some(project.is_git_repo),
        repo_root: Some(project.root.to_string_lossy().to_string()),
        branch: project.git_branch.clone(),
        request_repo_context: Some(true),
    }
}

/// Build a context header for including in the system prompt
pub fn build_context_header(project: &DetectedProject) -> String {
    let mut parts = vec![];

    parts.push(format!("Project: {}", project.name));

    if let Some(ref branch) = project.git_branch {
        parts.push(format!("Branch: {}", branch));
    }

    parts.push(format!("Root: {}", project.root.display()));

    parts.join(" | ")
}

/// Format MIRA.md content for injection into context
pub fn format_mira_md(content: &str) -> String {
    format!(
        "=== PROJECT INSTRUCTIONS (from MIRA.md) ===\n{}\n=== END PROJECT INSTRUCTIONS ===",
        content.trim()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn create_test_project() -> DetectedProject {
        DetectedProject {
            root: PathBuf::from("/home/user/my-project"),
            is_git_repo: true,
            git_branch: Some("main".to_string()),
            mira_md: Some("# My Project\n\nInstructions here.".to_string()),
            settings: None,
            name: "my-project".to_string(),
        }
    }

    #[test]
    fn test_build_metadata() {
        let project = create_test_project();
        let metadata = build_metadata(&project);

        assert_eq!(metadata.project_name, Some("my-project".to_string()));
        assert_eq!(metadata.has_repository, Some(true));
        assert_eq!(metadata.branch, Some("main".to_string()));
        assert!(metadata.repo_root.is_some());
    }

    #[test]
    fn test_build_context_header() {
        let project = create_test_project();
        let header = build_context_header(&project);

        assert!(header.contains("my-project"));
        assert!(header.contains("main"));
    }

    #[test]
    fn test_format_mira_md() {
        let content = "# Project\n\nSome instructions.";
        let formatted = format_mira_md(content);

        assert!(formatted.contains("PROJECT INSTRUCTIONS"));
        assert!(formatted.contains("Some instructions"));
    }
}
