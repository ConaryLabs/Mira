// crates/mira-server/src/git.rs
// Git operations using git2 crate

use git2::Repository;
use std::path::Path;
use std::sync::{LazyLock, RwLock};
use std::time::{Duration, Instant};

/// Cache for git branch to avoid repeated repository lookups
/// Caches for a short duration (5-10 seconds) since branch can change mid-session
struct BranchCache {
    branch: Option<String>,
    project_path: String,
    cached_at: Instant,
}

/// Default cache TTL in seconds
const BRANCH_CACHE_TTL_SECS: u64 = 5;

static BRANCH_CACHE: LazyLock<RwLock<Option<BranchCache>>> = LazyLock::new(|| RwLock::new(None));

/// Get the current git branch for a project path.
///
/// Uses git2 crate which handles:
/// - Git worktrees
/// - Submodules
/// - Bare repos
/// - Detached HEAD (normalized to "detached")
///
/// Returns None if:
/// - Path is not in a git repository
/// - Repository has no HEAD (empty repo)
/// - Any git operation fails
pub fn get_git_branch(project_path: &str) -> Option<String> {
    // Try to get from cache first
    {
        let cache = BRANCH_CACHE.read().ok()?;
        if let Some(ref cached) = *cache {
            if cached.project_path == project_path
                && cached.cached_at.elapsed() < Duration::from_secs(BRANCH_CACHE_TTL_SECS)
            {
                return cached.branch.clone();
            }
        }
    }

    // Cache miss or stale - fetch from git
    let branch = get_git_branch_uncached(project_path);

    // Update cache
    if let Ok(mut cache) = BRANCH_CACHE.write() {
        *cache = Some(BranchCache {
            branch: branch.clone(),
            project_path: project_path.to_string(),
            cached_at: Instant::now(),
        });
    }

    branch
}

/// Get git branch without caching (for internal use and testing)
pub fn get_git_branch_uncached(project_path: &str) -> Option<String> {
    let path = Path::new(project_path);

    // Repository::discover walks up the directory tree to find .git
    // This handles worktrees and submodules correctly
    let repo = Repository::discover(path).ok()?;

    // Check for detached HEAD
    if repo.head_detached().unwrap_or(false) {
        // Normalize detached HEAD to just "detached" to avoid ephemeral SHA clutter
        return Some("detached".to_string());
    }

    // Get HEAD reference
    let head = repo.head().ok()?;

    // shorthand() returns the branch name (e.g., "main", "feature-x")
    head.shorthand().map(|s| s.to_string())
}

/// Clear the branch cache (useful when we know the branch has changed)
pub fn clear_branch_cache() {
    if let Ok(mut cache) = BRANCH_CACHE.write() {
        *cache = None;
    }
}

/// Check if a path is inside a git repository
pub fn is_git_repo(project_path: &str) -> bool {
    let path = Path::new(project_path);
    Repository::discover(path).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::TempDir;

    fn init_git_repo(dir: &Path) {
        Command::new("git")
            .args(["init"])
            .current_dir(dir)
            .output()
            .expect("Failed to init git repo");

        // Configure git user for commits
        Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(dir)
            .output()
            .expect("Failed to configure git email");

        Command::new("git")
            .args(["config", "user.name", "Test User"])
            .current_dir(dir)
            .output()
            .expect("Failed to configure git name");
    }

    fn create_commit(dir: &Path, message: &str) {
        // Create a file to commit
        std::fs::write(dir.join("test.txt"), message).unwrap();

        Command::new("git")
            .args(["add", "."])
            .current_dir(dir)
            .output()
            .expect("Failed to add files");

        Command::new("git")
            .args(["commit", "-m", message])
            .current_dir(dir)
            .output()
            .expect("Failed to create commit");
    }

    #[test]
    fn test_non_git_directory() {
        let temp_dir = TempDir::new().unwrap();
        let result = get_git_branch_uncached(temp_dir.path().to_str().unwrap());
        assert!(result.is_none());
    }

    #[test]
    fn test_empty_git_repo() {
        let temp_dir = TempDir::new().unwrap();
        init_git_repo(temp_dir.path());

        // Empty repo has no HEAD yet
        let result = get_git_branch_uncached(temp_dir.path().to_str().unwrap());
        assert!(result.is_none());
    }

    #[test]
    fn test_main_branch() {
        let temp_dir = TempDir::new().unwrap();
        init_git_repo(temp_dir.path());
        create_commit(temp_dir.path(), "Initial commit");

        let result = get_git_branch_uncached(temp_dir.path().to_str().unwrap());
        // Git defaults to either "main" or "master" depending on config
        assert!(result.is_some());
        let branch = result.unwrap();
        assert!(branch == "main" || branch == "master");
    }

    #[test]
    fn test_feature_branch() {
        let temp_dir = TempDir::new().unwrap();
        init_git_repo(temp_dir.path());
        create_commit(temp_dir.path(), "Initial commit");

        // Create and checkout feature branch
        Command::new("git")
            .args(["checkout", "-b", "feature-test"])
            .current_dir(temp_dir.path())
            .output()
            .expect("Failed to create branch");

        let result = get_git_branch_uncached(temp_dir.path().to_str().unwrap());
        assert_eq!(result, Some("feature-test".to_string()));
    }

    #[test]
    fn test_detached_head() {
        let temp_dir = TempDir::new().unwrap();
        init_git_repo(temp_dir.path());
        create_commit(temp_dir.path(), "First commit");
        create_commit(temp_dir.path(), "Second commit");

        // Checkout a specific commit to get detached HEAD
        Command::new("git")
            .args(["checkout", "HEAD~1"])
            .current_dir(temp_dir.path())
            .output()
            .expect("Failed to checkout commit");

        let result = get_git_branch_uncached(temp_dir.path().to_str().unwrap());
        assert_eq!(result, Some("detached".to_string()));
    }

    #[test]
    fn test_is_git_repo() {
        let temp_dir = TempDir::new().unwrap();

        // Not a git repo yet
        assert!(!is_git_repo(temp_dir.path().to_str().unwrap()));

        // Initialize git
        init_git_repo(temp_dir.path());

        // Now it's a git repo
        assert!(is_git_repo(temp_dir.path().to_str().unwrap()));
    }

    #[test]
    fn test_cache_behavior() {
        let temp_dir = TempDir::new().unwrap();
        init_git_repo(temp_dir.path());
        create_commit(temp_dir.path(), "Initial commit");

        let path = temp_dir.path().to_str().unwrap();

        // Clear cache first
        clear_branch_cache();

        // First call should populate cache
        let result1 = get_git_branch(path);
        assert!(result1.is_some());

        // Second call should use cache (same result)
        let result2 = get_git_branch(path);
        assert_eq!(result1, result2);
    }
}
