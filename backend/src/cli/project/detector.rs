// backend/src/cli/project/detector.rs
// Project detection for CLI - finds git repos, MIRA.md, .mira/ directories

use anyhow::Result;
use std::path::{Path, PathBuf};

/// Detected project information
#[derive(Debug, Clone)]
pub struct DetectedProject {
    /// Root directory of the project
    pub root: PathBuf,
    /// Whether this is a git repository
    pub is_git_repo: bool,
    /// Current git branch (if applicable)
    pub git_branch: Option<String>,
    /// Contents of MIRA.md (if found)
    pub mira_md: Option<String>,
    /// Contents of .mira/settings.json (if found)
    pub settings: Option<String>,
    /// Project name (from directory name or git remote)
    pub name: String,
}

impl DetectedProject {
    /// Check if this project has any special configuration
    pub fn has_config(&self) -> bool {
        self.mira_md.is_some() || self.settings.is_some()
    }
}

/// Project detector
pub struct ProjectDetector;

impl ProjectDetector {
    /// Detect project from the current working directory
    pub fn detect() -> Result<Option<DetectedProject>> {
        let cwd = std::env::current_dir()?;
        Self::detect_from(&cwd)
    }

    /// Detect project from a specific path
    pub fn detect_from(start_path: &Path) -> Result<Option<DetectedProject>> {
        // Find the project root (git root or directory with .mira/)
        let project_root = Self::find_project_root(start_path);

        match project_root {
            Some(root) => {
                let is_git_repo = root.join(".git").exists();
                let git_branch = if is_git_repo {
                    Self::get_git_branch(&root)
                } else {
                    None
                };

                let mira_md = Self::load_mira_md(&root);
                let settings = Self::load_mira_settings(&root);

                let name = Self::detect_project_name(&root, is_git_repo);

                Ok(Some(DetectedProject {
                    root,
                    is_git_repo,
                    git_branch,
                    mira_md,
                    settings,
                    name,
                }))
            }
            None => Ok(None),
        }
    }

    /// Find the project root by walking up the directory tree
    fn find_project_root(start: &Path) -> Option<PathBuf> {
        let mut current = start.to_path_buf();

        loop {
            // Check for indicators of a project root
            if current.join(".git").exists()
                || current.join(".mira").exists()
                || current.join("MIRA.md").exists()
                || current.join("CLAUDE.md").exists()
            {
                return Some(current);
            }

            // Also check for common project files
            if current.join("Cargo.toml").exists()
                || current.join("package.json").exists()
                || current.join("pyproject.toml").exists()
                || current.join("go.mod").exists()
            {
                return Some(current);
            }

            // Move up one directory
            if !current.pop() {
                break;
            }
        }

        None
    }

    /// Get the current git branch
    fn get_git_branch(repo_path: &Path) -> Option<String> {
        // Try to read .git/HEAD
        let head_path = repo_path.join(".git/HEAD");
        if let Ok(content) = std::fs::read_to_string(&head_path) {
            let content = content.trim();
            if content.starts_with("ref: refs/heads/") {
                return Some(content.trim_start_matches("ref: refs/heads/").to_string());
            }
            // Detached HEAD - return short hash
            if content.len() >= 7 {
                return Some(content[..7].to_string());
            }
        }
        None
    }

    /// Load MIRA.md or CLAUDE.md content
    fn load_mira_md(root: &Path) -> Option<String> {
        // Try MIRA.md first, then CLAUDE.md
        for name in &["MIRA.md", "CLAUDE.md"] {
            let path = root.join(name);
            if path.exists() {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    return Some(content);
                }
            }
        }

        // Also check .mira/MIRA.md
        let mira_dir_path = root.join(".mira/MIRA.md");
        if mira_dir_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&mira_dir_path) {
                return Some(content);
            }
        }

        None
    }

    /// Load .mira/settings.json
    fn load_mira_settings(root: &Path) -> Option<String> {
        let settings_path = root.join(".mira/settings.json");
        if settings_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&settings_path) {
                return Some(content);
            }
        }
        None
    }

    /// Detect the project name
    fn detect_project_name(root: &Path, is_git_repo: bool) -> String {
        // Try to get name from git remote
        if is_git_repo {
            if let Some(name) = Self::get_git_remote_name(root) {
                return name;
            }
        }

        // Fall back to directory name
        root.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Unknown")
            .to_string()
    }

    /// Get project name from git remote URL
    fn get_git_remote_name(repo_path: &Path) -> Option<String> {
        let config_path = repo_path.join(".git/config");
        if let Ok(content) = std::fs::read_to_string(&config_path) {
            // Simple regex-free parsing for remote URL
            for line in content.lines() {
                let line = line.trim();
                if line.starts_with("url = ") {
                    let url = line.trim_start_matches("url = ");
                    return Self::extract_repo_name(url);
                }
            }
        }
        None
    }

    /// Extract repository name from a git URL
    fn extract_repo_name(url: &str) -> Option<String> {
        // Handle SSH URLs: git@github.com:user/repo.git
        if url.contains(':') && !url.contains("://") {
            let parts: Vec<&str> = url.split(':').collect();
            if parts.len() == 2 {
                let repo_path = parts[1].trim_end_matches(".git");
                if let Some(name) = repo_path.split('/').last() {
                    return Some(name.to_string());
                }
            }
        }

        // Handle HTTPS URLs: https://github.com/user/repo.git
        if let Some(path) = url.strip_prefix("https://") {
            let path = path.trim_end_matches(".git");
            if let Some(name) = path.split('/').last() {
                return Some(name.to_string());
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_extract_repo_name_ssh() {
        assert_eq!(
            ProjectDetector::extract_repo_name("git@github.com:user/my-repo.git"),
            Some("my-repo".to_string())
        );
    }

    #[test]
    fn test_extract_repo_name_https() {
        assert_eq!(
            ProjectDetector::extract_repo_name("https://github.com/user/my-repo.git"),
            Some("my-repo".to_string())
        );
        assert_eq!(
            ProjectDetector::extract_repo_name("https://github.com/user/my-repo"),
            Some("my-repo".to_string())
        );
    }

    #[test]
    fn test_find_project_root_git() {
        let temp_dir = TempDir::new().unwrap();
        let git_dir = temp_dir.path().join(".git");
        std::fs::create_dir(&git_dir).unwrap();

        let subdir = temp_dir.path().join("src").join("deep");
        std::fs::create_dir_all(&subdir).unwrap();

        let root = ProjectDetector::find_project_root(&subdir);
        assert_eq!(root, Some(temp_dir.path().to_path_buf()));
    }

    #[test]
    fn test_find_project_root_mira() {
        let temp_dir = TempDir::new().unwrap();
        let mira_dir = temp_dir.path().join(".mira");
        std::fs::create_dir(&mira_dir).unwrap();

        let root = ProjectDetector::find_project_root(temp_dir.path());
        assert_eq!(root, Some(temp_dir.path().to_path_buf()));
    }

    #[test]
    fn test_load_mira_md() {
        let temp_dir = TempDir::new().unwrap();
        let mira_md = temp_dir.path().join("MIRA.md");
        std::fs::write(&mira_md, "# Test Project\n\nInstructions here.").unwrap();

        let content = ProjectDetector::load_mira_md(temp_dir.path());
        assert!(content.is_some());
        assert!(content.unwrap().contains("Test Project"));
    }

    #[test]
    fn test_detect_project_name_fallback() {
        let temp_dir = TempDir::new().unwrap();
        let name = ProjectDetector::detect_project_name(temp_dir.path(), false);
        // Should return the temp directory name
        assert!(!name.is_empty());
    }
}
