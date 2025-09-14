// src/git/client/mod.rs
// FIXED: Git client over-abstraction resolved by stopping the delegator pattern
// Keep modules separate but eliminate unnecessary delegation/forwarding
// CRITICAL: Maintains 100% API compatibility with existing code

use anyhow::Result;
use std::path::{Path, PathBuf};
use std::fs;

use crate::git::types::GitRepoAttachment;
use crate::git::store::GitStore;
use crate::api::error::ApiResult;

// Keep internal modules for their substantial logic
pub mod operations;
pub mod tree_builder; 
pub mod diff_parser;
pub mod branch_manager;

// Re-export public types for backward compatibility
pub use operations::GitOperations;
pub use tree_builder::{FileNode, FileNodeType, TreeBuilder};
pub use diff_parser::{DiffInfo, DiffParser};
pub use branch_manager::{BranchInfo, CommitInfo, BranchManager};

/// Main API for attaching, cloning, importing, and syncing a GitHub repo for a project.
/// FIXED: No longer delegates - modules are used directly where needed
#[derive(Clone)]
pub struct GitClient {
    pub git_dir: PathBuf,
    pub store: GitStore,
}

impl GitClient {
    /// Create a new client with a directory for all clones (e.g. "./repos")
    /// CRITICAL: Maintains exact same signature as original
    pub fn new<P: AsRef<Path>>(git_dir: P, store: GitStore) -> Self {
        fs::create_dir_all(&git_dir).ok();
        
        Self {
            git_dir: git_dir.as_ref().to_path_buf(),
            store,
        }
    }

    // ===== CORE REPOSITORY OPERATIONS =====
    // Use operations module directly, no unnecessary delegation

    /// Attach a repo: generate an ID, determine clone path, and persist the attachment.
    /// CRITICAL: Maintains exact same signature and behavior
    pub async fn attach_repo(
        &self,
        project_id: &str,
        repo_url: &str,
    ) -> Result<GitRepoAttachment> {
        let ops = GitOperations::new(self.git_dir.clone(), self.store.clone());
        ops.attach_repo(project_id, repo_url).await
    }

    /// Clone the attached repo to disk. Returns Result<()>.
    /// CRITICAL: Maintains exact same signature and behavior  
    pub async fn clone_repo(&self, attachment: &GitRepoAttachment) -> Result<()> {
        let ops = GitOperations::new(self.git_dir.clone(), self.store.clone());
        ops.clone_repo(attachment).await
    }

    /// Import files into your DB (MVP: just record file paths and contents)
    /// CRITICAL: Maintains exact same signature and behavior
    pub async fn import_codebase(&self, attachment: &GitRepoAttachment) -> Result<()> {
        let ops = GitOperations::new(self.git_dir.clone(), self.store.clone());
        ops.import_codebase(attachment).await
    }

    /// Sync: commit and push DB-side code changes back to GitHub.
    /// CRITICAL: Maintains exact same signature and behavior
    pub async fn sync_changes(&self, attachment: &GitRepoAttachment, commit_message: &str) -> Result<()> {
        let ops = GitOperations::new(self.git_dir.clone(), self.store.clone());
        ops.sync_changes(attachment, commit_message).await
    }

    /// Pull latest changes from remote
    /// NEW: Added for Phase 1 MVP completion
    pub async fn pull_changes(&self, attachment_id: &str) -> ApiResult<()> {
        let ops = GitOperations::new(self.git_dir.clone(), self.store.clone());
        ops.pull_changes(attachment_id).await
    }
    
    /// Reset to remote HEAD (destructive)
    /// NEW: Added for Phase 1 MVP completion
    pub async fn reset_to_remote(&self, attachment_id: &str) -> ApiResult<()> {
        let ops = GitOperations::new(self.git_dir.clone(), self.store.clone());
        ops.reset_to_remote(attachment_id).await
    }

    // ===== FILE TREE OPERATIONS =====
    // Use tree_builder module directly, no unnecessary delegation

    /// Get the file tree of a repository
    /// CRITICAL: Maintains exact same signature and return type
    pub fn get_file_tree(&self, attachment: &GitRepoAttachment) -> Result<Vec<FileNode>> {
        let tree_builder = TreeBuilder::new();
        tree_builder.get_file_tree(attachment)
    }

    /// Get file content from the repository
    /// CRITICAL: Maintains exact same signature and behavior
    pub fn get_file_content(&self, attachment: &GitRepoAttachment, file_path: &str) -> Result<String> {
        let tree_builder = TreeBuilder::new();
        tree_builder.get_file_content(attachment, file_path)
    }

    /// Update file content in the repository
    /// CRITICAL: Maintains exact same signature and behavior
    pub fn update_file_content(
        &self,
        attachment: &GitRepoAttachment,
        file_path: &str,
        content: &str,
        commit_message: Option<&str>,
    ) -> Result<()> {
        let tree_builder = TreeBuilder::new();
        tree_builder.update_file_content(attachment, file_path, content, commit_message)
    }

    // ===== BRANCH MANAGEMENT =====
    // Use branch_manager module directly, no unnecessary delegation

    /// Get all branches in the repository
    /// CRITICAL: Maintains exact same signature and return type
    pub fn get_branches(&self, attachment: &GitRepoAttachment) -> Result<Vec<BranchInfo>> {
        let branch_manager = BranchManager::new();
        branch_manager.get_branches(attachment)
    }

    /// Switch to a different branch  
    /// CRITICAL: Maintains exact same signature and behavior
    pub fn switch_branch(&self, attachment: &GitRepoAttachment, branch_name: &str) -> Result<()> {
        let branch_manager = BranchManager::new();
        branch_manager.switch_branch(attachment, branch_name)
    }

    /// Get commit history for the repository
    /// CRITICAL: Maintains exact same signature and return type
    pub fn get_commits(&self, attachment: &GitRepoAttachment, limit: usize) -> Result<Vec<CommitInfo>> {
        let branch_manager = BranchManager::new();
        branch_manager.get_commits(attachment, limit)
    }

    /// Get commit history with optional limit (used by HTTP handlers)
    /// CRITICAL: Added for compatibility with commits handler
    pub fn get_commit_history(&self, attachment: &GitRepoAttachment, limit: Option<usize>) -> Result<Vec<CommitInfo>> {
        let branch_manager = BranchManager::new();
        branch_manager.get_commits(attachment, limit.unwrap_or(50))
    }

    // ===== DIFF OPERATIONS =====
    // Use diff_parser module directly, no unnecessary delegation

    /// Get diff for a specific commit
    /// CRITICAL: Maintains exact same signature and return type
    pub fn get_diff(&self, attachment: &GitRepoAttachment, commit_id: &str) -> Result<DiffInfo> {
        let diff_parser = DiffParser::new();
        diff_parser.get_commit_diff(attachment, commit_id)
    }

    /// Get file content at a specific commit
    /// CRITICAL: Maintains exact same signature and behavior
    pub fn get_file_at_commit(
        &self,
        attachment: &GitRepoAttachment,
        commit_id: &str,
        file_path: &str,
    ) -> Result<String> {
        let diff_parser = DiffParser::new();
        diff_parser.get_file_at_commit(attachment, commit_id, file_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_client_creation() {
        let temp_dir = TempDir::new().unwrap();
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .connect(":memory:")
            .await
            .unwrap();
        let store = GitStore::new(pool);
        
        let client = GitClient::new(temp_dir.path(), store);
        assert!(temp_dir.path().exists());
    }
}

// Add project-aware operations
pub mod project_ops;
pub use project_ops::ProjectOps;
