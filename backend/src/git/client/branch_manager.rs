// src/git/client/branch_manager.rs
// Branch and commit management operations
// Extracted from monolithic GitClient for focused responsibility

use anyhow::Result;
use chrono::{DateTime, Utc};
use git2::{BranchType, Oid, Repository};
use serde::{Deserialize, Serialize};

use crate::git::error::IntoGitErrorResult;
use crate::git::types::GitRepoAttachment;

/// Branch information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchInfo {
    pub name: String,
    pub is_head: bool,
    pub commit_id: String,
    pub commit_message: String,
}

/// Commit information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitInfo {
    pub id: String,
    pub message: String,
    pub author_name: String,
    pub author_email: String,
    pub timestamp: DateTime<Utc>,
    pub parent_ids: Vec<String>,
}

/// Handles branch and commit operations
#[derive(Clone)]
pub struct BranchManager;

impl Default for BranchManager {
    fn default() -> Self {
        Self::new()
    }
}

impl BranchManager {
    /// Create new branch manager
    pub fn new() -> Self {
        Self
    }

    /// Get all branches in the repository
    pub fn get_branches(&self, attachment: &GitRepoAttachment) -> Result<Vec<BranchInfo>> {
        let repo =
            Repository::open(&attachment.local_path).into_git_error("Failed to open repository")?;

        let mut branches = Vec::new();

        // Get current head
        let head_ref = repo.head().ok();
        let current_branch_name = head_ref.as_ref().and_then(|r| r.shorthand()).unwrap_or("");

        // Iterate through all branches
        let git_branches = repo
            .branches(Some(BranchType::Local))
            .into_git_error("Failed to get branches")?;

        for branch_result in git_branches {
            let (branch, _branch_type) =
                branch_result.into_git_error("Failed to process branch")?;

            if let Some(name) = branch.name().into_git_error("Failed to get branch name")? {
                let is_head = name == current_branch_name;

                // Get the commit this branch points to
                let commit = branch
                    .get()
                    .peel_to_commit()
                    .into_git_error("Failed to get branch commit")?;

                let commit_id = commit.id().to_string();
                let commit_message = commit.message().unwrap_or("").to_string();

                branches.push(BranchInfo {
                    name: name.to_string(),
                    is_head,
                    commit_id,
                    commit_message,
                });
            }
        }

        Ok(branches)
    }

    /// Switch to a different branch
    pub fn switch_branch(&self, attachment: &GitRepoAttachment, branch_name: &str) -> Result<()> {
        let repo =
            Repository::open(&attachment.local_path).into_git_error("Failed to open repository")?;

        // Find the branch
        let branch = repo
            .find_branch(branch_name, BranchType::Local)
            .into_git_error("Branch not found")?;

        // Get the commit that the branch points to
        let commit = branch
            .get()
            .peel_to_commit()
            .into_git_error("Failed to get branch commit")?;

        // Set HEAD to point to this branch
        repo.set_head(&format!("refs/heads/{branch_name}"))
            .into_git_error("Failed to set HEAD")?;

        // Checkout the commit
        repo.checkout_tree(commit.as_object(), None)
            .into_git_error("Failed to checkout branch")?;

        Ok(())
    }

    /// Get commit history for the repository
    pub fn get_commits(
        &self,
        attachment: &GitRepoAttachment,
        limit: usize,
    ) -> Result<Vec<CommitInfo>> {
        let repo =
            Repository::open(&attachment.local_path).into_git_error("Failed to open repository")?;

        let mut revwalk = repo.revwalk().into_git_error("Failed to create revwalk")?;

        revwalk
            .push_head()
            .into_git_error("Failed to push HEAD to revwalk")?;

        let mut commits = Vec::new();
        let mut count = 0;

        for oid in revwalk {
            if count >= limit {
                break;
            }

            let oid = oid.into_git_error("Failed to get commit OID")?;
            let commit = repo
                .find_commit(oid)
                .into_git_error("Failed to find commit")?;

            let commit_info = self.commit_to_info(&repo, &commit)?;
            commits.push(commit_info);
            count += 1;
        }

        Ok(commits)
    }

    /// Get commit history starting from a specific commit
    pub fn get_commits_from(
        &self,
        attachment: &GitRepoAttachment,
        commit_id: &str,
        limit: usize,
    ) -> Result<Vec<CommitInfo>> {
        let repo =
            Repository::open(&attachment.local_path).into_git_error("Failed to open repository")?;

        let oid = Oid::from_str(commit_id).into_git_error("Invalid commit ID")?;

        let mut revwalk = repo.revwalk().into_git_error("Failed to create revwalk")?;

        revwalk
            .push(oid)
            .into_git_error("Failed to push commit to revwalk")?;

        let mut commits = Vec::new();
        let mut count = 0;

        for oid in revwalk {
            if count >= limit {
                break;
            }

            let oid = oid.into_git_error("Failed to get commit OID")?;
            let commit = repo
                .find_commit(oid)
                .into_git_error("Failed to find commit")?;

            let commit_info = self.commit_to_info(&repo, &commit)?;
            commits.push(commit_info);
            count += 1;
        }

        Ok(commits)
    }

    /// Get specific commit by ID
    pub fn get_commit(
        &self,
        attachment: &GitRepoAttachment,
        commit_id: &str,
    ) -> Result<CommitInfo> {
        let repo =
            Repository::open(&attachment.local_path).into_git_error("Failed to open repository")?;

        let oid = Oid::from_str(commit_id).into_git_error("Invalid commit ID")?;

        let commit = repo.find_commit(oid).into_git_error("Commit not found")?;

        self.commit_to_info(&repo, &commit)
    }

    /// Convert git2::Commit to CommitInfo
    fn commit_to_info(&self, _repo: &Repository, commit: &git2::Commit) -> Result<CommitInfo> {
        let author = commit.author();
        let author_name = author.name().unwrap_or("").to_string();
        let author_email = author.email().unwrap_or("").to_string();

        let timestamp =
            DateTime::from_timestamp(author.when().seconds(), 0).unwrap_or_else(Utc::now);

        let parent_ids: Vec<String> = (0..commit.parent_count())
            .map(|i| commit.parent_id(i).map(|oid| oid.to_string()))
            .collect::<Result<Vec<_>, _>>()
            .into_git_error("Failed to get parent IDs")?;

        Ok(CommitInfo {
            id: commit.id().to_string(),
            message: commit.message().unwrap_or("").to_string(),
            author_name,
            author_email,
            timestamp,
            parent_ids,
        })
    }
}
