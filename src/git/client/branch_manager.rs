// src/git/client/branch_manager.rs
// Branch management and commit history operations
// Extracted from monolithic GitClient for focused responsibility

use anyhow::{Result, Context};
use git2::{Repository, BranchType, Oid};
use serde::{Serialize, Deserialize};

use crate::git::types::GitRepoAttachment;
use crate::api::error::IntoApiError;

/// Branch information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchInfo {
    pub name: String,
    pub is_current: bool,
    pub last_commit_id: String,
    pub last_commit_message: String,
    pub last_commit_time: i64,
}

/// Commit information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitInfo {
    pub id: String,
    pub message: String,
    pub author: String,
    pub email: String,
    pub timestamp: i64,
    pub parent_ids: Vec<String>,
}

/// Handles branch management and commit history
#[derive(Clone)]
pub struct BranchManager;

impl BranchManager {
    /// Create new branch manager
    pub fn new() -> Self {
        Self
    }

    /// Get all branches in the repository
    pub fn get_branches(&self, attachment: &GitRepoAttachment) -> Result<Vec<BranchInfo>> {
        let repo = Repository::open(&attachment.local_path)
            .into_api_error("Failed to open repository")?;

        let mut branches = Vec::new();
        
        // Get current branch name
        let current_branch_name = get_current_branch_name(&repo)?;

        let branch_iter = repo.branches(Some(BranchType::Local))
            .into_api_error("Failed to get branches")?;

        for branch_result in branch_iter {
            let (branch, _) = branch_result
                .into_api_error("Failed to access branch")?;

            if let Some(name) = branch.name()
                .into_api_error("Failed to get branch name")?
            {
                let is_current = Some(name) == current_branch_name.as_deref();
                let commit = branch.get().peel_to_commit()
                    .into_api_error("Failed to get branch commit")?;

                branches.push(BranchInfo {
                    name: name.to_string(),
                    is_current,
                    last_commit_id: commit.id().to_string(),
                    last_commit_message: commit.message().unwrap_or("").to_string(),
                    last_commit_time: commit.time().seconds(),
                });
            }
        }

        // Sort branches: current first, then alphabetically
        branches.sort_by(|a, b| {
            match (a.is_current, b.is_current) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.name.cmp(&b.name),
            }
        });

        Ok(branches)
    }

    /// Switch to a different branch
    pub fn switch_branch(&self, attachment: &GitRepoAttachment, branch_name: &str) -> Result<()> {
        let repo = Repository::open(&attachment.local_path)
            .into_api_error("Failed to open repository")?;
        
        // Find the branch
        let branch = repo.find_branch(branch_name, BranchType::Local)
            .into_api_error(&format!("Branch '{}' not found", branch_name))?;
        
        // Set HEAD to the branch
        repo.set_head(branch.get().name().unwrap())
            .into_api_error("Failed to set HEAD")?;
        
        // Checkout the branch
        repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
            .into_api_error("Failed to checkout branch")?;
        
        tracing::info!("Switched to branch: {}", branch_name);
        Ok(())
    }

    /// Create a new branch
    pub fn create_branch(&self, attachment: &GitRepoAttachment, branch_name: &str) -> Result<()> {
        let repo = Repository::open(&attachment.local_path)
            .into_api_error("Failed to open repository")?;

        let head_commit = repo.head()
            .into_api_error("Failed to get HEAD")?
            .peel_to_commit()
            .into_api_error("Failed to get HEAD commit")?;

        repo.branch(branch_name, &head_commit, false)
            .into_api_error("Failed to create branch")?;

        tracing::info!("Created new branch: {}", branch_name);
        Ok(())
    }

    /// Delete a branch
    pub fn delete_branch(&self, attachment: &GitRepoAttachment, branch_name: &str) -> Result<()> {
        let repo = Repository::open(&attachment.local_path)
            .into_api_error("Failed to open repository")?;

        let mut branch = repo.find_branch(branch_name, BranchType::Local)
            .into_api_error("Failed to find branch")?;

        // Prevent deleting the current branch
        if let Ok(Some(current)) = get_current_branch_name(&repo) {
            if current == branch_name {
                return Err(anyhow::anyhow!("Cannot delete the current branch"));
            }
        }

        branch.delete()
            .into_api_error("Failed to delete branch")?;

        tracing::info!("Deleted branch: {}", branch_name);
        Ok(())
    }

    /// Get commit history for the repository
    pub fn get_commits(&self, attachment: &GitRepoAttachment, limit: usize) -> Result<Vec<CommitInfo>> {
        let repo = Repository::open(&attachment.local_path)
            .into_api_error("Failed to open repository")?;
        
        let mut revwalk = repo.revwalk()
            .into_api_error("Failed to create revision walker")?;
        
        revwalk.push_head()
            .into_api_error("Failed to push HEAD to revwalk")?;
        
        revwalk.set_sorting(git2::Sort::TIME)
            .into_api_error("Failed to set sorting")?;

        let commits: Result<Vec<_>> = revwalk
            .take(limit)
            .map(|oid_result| {
                let oid = oid_result
                    .into_api_error("Failed to get commit OID")?;
                let commit = repo.find_commit(oid)
                    .into_api_error("Failed to find commit")?;
                
                commit_to_info(&commit)
            })
            .collect();
        
        commits
    }

    /// Get commit by ID
    pub fn get_commit(&self, attachment: &GitRepoAttachment, commit_id: &str) -> Result<CommitInfo> {
        let repo = Repository::open(&attachment.local_path)
            .into_api_error("Failed to open repository")?;

        let oid = Oid::from_str(commit_id)
            .into_api_error("Invalid commit ID")?;

        let commit = repo.find_commit(oid)
            .into_api_error("Commit not found")?;

        commit_to_info(&commit)
    }

    /// Check if repository has any commits
    pub fn has_commits(&self, attachment: &GitRepoAttachment) -> Result<bool> {
        let repo = Repository::open(&attachment.local_path)
            .into_api_error("Failed to open repository")?;

        match repo.head() {
            Ok(_) => Ok(true),
            Err(e) if e.code() == git2::ErrorCode::UnbornBranch => Ok(false),
            Err(e) => Err(anyhow::anyhow!("Failed to check for commits: {}", e)),
        }
    }

    /// Get commits for a specific branch
    pub fn get_branch_commits(
        &self,
        attachment: &GitRepoAttachment,
        branch_name: &str,
        limit: usize,
    ) -> Result<Vec<CommitInfo>> {
        let repo = Repository::open(&attachment.local_path)
            .into_api_error("Failed to open repository")?;

        let branch = repo.find_branch(branch_name, BranchType::Local)
            .into_api_error("Failed to find branch")?;

        let mut revwalk = repo.revwalk()
            .into_api_error("Failed to create revision walker")?;

        let branch_oid = branch.get().target()
            .ok_or_else(|| anyhow::anyhow!("Branch has no target"))?;

        revwalk.push(branch_oid)
            .into_api_error("Failed to push branch to revwalk")?;

        revwalk.set_sorting(git2::Sort::TIME)
            .into_api_error("Failed to set sorting")?;

        let commits: Result<Vec<_>> = revwalk
            .take(limit)
            .map(|oid_result| {
                let oid = oid_result
                    .into_api_error("Failed to get commit OID")?;
                let commit = repo.find_commit(oid)
                    .into_api_error("Failed to find commit")?;

                commit_to_info(&commit)
            })
            .collect();

        commits
    }
}

/// Get the name of the current branch
fn get_current_branch_name(repo: &Repository) -> Result<Option<String>> {
    match repo.head() {
        Ok(head) => {
            if head.is_branch() {
                Ok(head.shorthand().map(String::from))
            } else {
                Ok(None) // Detached HEAD
            }
        }
        Err(_) => Ok(None), // No HEAD (empty repo)
    }
}

/// Convert a git2::Commit to CommitInfo
fn commit_to_info(commit: &git2::Commit) -> Result<CommitInfo> {
    let id = commit.id().to_string();
    let message = commit.message().unwrap_or("").to_string();
    let author = commit.author();
    let author_name = author.name().unwrap_or("Unknown").to_string();
    let email = author.email().unwrap_or("").to_string();
    let timestamp = commit.time().seconds();

    let parent_ids: Vec<String> = (0..commit.parent_count())
        .map(|i| commit.parent_id(i).map(|oid| oid.to_string()))
        .collect::<Result<Vec<_>, _>>()
        .into_api_error("Failed to get parent IDs")?;

    Ok(CommitInfo {
        id,
        message,
        author: author_name,
        email,
        timestamp,
        parent_ids,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_branch_manager_creation() {
        let manager = BranchManager::new();
        // BranchManager is stateless, so just verify it can be created
        assert!(std::mem::size_of_val(&manager) >= 0);
    }

    #[test]
    fn test_commit_info_serialization() {
        let commit_info = CommitInfo {
            id: "abcd1234".to_string(),
            message: "Test commit".to_string(),
            author: "Test Author".to_string(),
            email: "test@example.com".to_string(),
            timestamp: 1234567890,
            parent_ids: vec!["parent1".to_string()],
        };

        let json = serde_json::to_string(&commit_info).unwrap();
        let deserialized: CommitInfo = serde_json::from_str(&json).unwrap();
        
        assert_eq!(commit_info.id, deserialized.id);
        assert_eq!(commit_info.message, deserialized.message);
        assert_eq!(commit_info.author, deserialized.author);
    }

    #[test]
    fn test_branch_info_serialization() {
        let branch_info = BranchInfo {
            name: "main".to_string(),
            is_current: true,
            last_commit_id: "abcd1234".to_string(),
            last_commit_message: "Initial commit".to_string(),
            last_commit_time: 1234567890,
        };

        let json = serde_json::to_string(&branch_info).unwrap();
        let deserialized: BranchInfo = serde_json::from_str(&json).unwrap();
        
        assert_eq!(branch_info.name, deserialized.name);
        assert_eq!(branch_info.is_current, deserialized.is_current);
    }
}
