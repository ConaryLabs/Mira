// src/git/client/mod.rs
use anyhow::Result;
use std::path::{Path, PathBuf};
use std::fs;

use crate::git::types::GitRepoAttachment;
use crate::git::store::GitStore;
use crate::api::error::{ApiResult, ApiError};
use crate::memory::features::code_intelligence::CodeIntelligenceService;

pub mod operations;
pub mod tree_builder; 
pub mod diff_parser;
pub mod branch_manager;
pub mod project_ops;
pub mod code_sync;  // NEW

pub use operations::GitOperations;
pub use tree_builder::{FileNode, FileNodeType, TreeBuilder};
pub use diff_parser::{DiffInfo, DiffParser};
pub use branch_manager::{BranchInfo, CommitInfo, BranchManager};
pub use project_ops::ProjectOps;
pub use code_sync::CodeSync;  // NEW

#[derive(Clone)]
pub struct GitClient {
    pub git_dir: PathBuf,
    pub store: GitStore,
    pub code_intelligence: Option<CodeIntelligenceService>,
}

impl GitClient {
    pub fn new<P: AsRef<Path>>(git_dir: P, store: GitStore) -> Self {
        fs::create_dir_all(&git_dir).ok();
        
        Self {
            git_dir: git_dir.as_ref().to_path_buf(),
            store,
            code_intelligence: None,
        }
    }

    pub fn with_code_intelligence<P: AsRef<Path>>(
        git_dir: P, 
        store: GitStore, 
        code_intelligence: CodeIntelligenceService
    ) -> Self {
        fs::create_dir_all(&git_dir).ok();
        
        Self {
            git_dir: git_dir.as_ref().to_path_buf(),
            store,
            code_intelligence: Some(code_intelligence),
        }
    }

    fn create_operations(&self) -> GitOperations {
        match &self.code_intelligence {
            Some(code_intel) => {
                let code_sync = CodeSync::new(
                    self.store.clone(),
                    code_intel.clone()
                );
                GitOperations::with_code_sync(
                    self.git_dir.clone(),
                    self.store.clone(),
                    code_sync
                )
            },
            None => GitOperations::new(self.git_dir.clone(), self.store.clone()),
        }
    }

    pub fn has_code_intelligence(&self) -> bool {
        self.code_intelligence.is_some()
    }

    // Core repository operations
    pub async fn attach_repo(&self, project_id: &str, repo_url: &str) -> Result<GitRepoAttachment> {
        let ops = self.create_operations();
        ops.attach_repo(project_id, repo_url).await
    }

    pub async fn clone_repo(&self, attachment: &GitRepoAttachment) -> Result<()> {
        let ops = self.create_operations();
        ops.clone_repo(attachment).await
    }

    pub async fn import_codebase(&self, attachment: &GitRepoAttachment) -> Result<()> {
        let ops = self.create_operations();
        ops.import_codebase(attachment).await
    }

    pub async fn sync_changes(&self, attachment: &GitRepoAttachment, commit_message: &str) -> Result<()> {
        let ops = self.create_operations();
        ops.sync_changes(attachment, commit_message).await
    }

    pub async fn pull_changes(&self, attachment_id: &str) -> ApiResult<()> {
        let ops = self.create_operations();
        
        // Fetch the attachment from store first
        let attachment = self.store
            .get_attachment(attachment_id)
            .await
            .map_err(|e| ApiError::internal(format!("Failed to get git attachment: {}", e)))?
            .ok_or_else(|| ApiError::not_found(format!("Git attachment not found: {}", attachment_id)))?;
        
        ops.pull_changes(&attachment)
            .await
            .map_err(|e| ApiError::internal(format!("Failed to pull changes: {}", e)))
    }
    
    pub async fn reset_to_remote(&self, attachment_id: &str) -> ApiResult<()> {
        let ops = self.create_operations();
        ops.reset_to_remote(attachment_id).await
    }

    // File tree operations
    pub fn get_file_tree(&self, attachment: &GitRepoAttachment) -> Result<Vec<FileNode>> {
        let tree_builder = TreeBuilder::new();
        tree_builder.get_file_tree(attachment)
    }

    pub fn get_file_content(&self, attachment: &GitRepoAttachment, file_path: &str) -> Result<String> {
        let tree_builder = TreeBuilder::new();
        tree_builder.get_file_content(attachment, file_path)
    }

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

    // Branch management
    pub fn get_branches(&self, attachment: &GitRepoAttachment) -> Result<Vec<BranchInfo>> {
        let branch_manager = BranchManager::new();
        branch_manager.get_branches(attachment)
    }

    pub fn switch_branch(&self, attachment: &GitRepoAttachment, branch_name: &str) -> Result<()> {
        let branch_manager = BranchManager::new();
        branch_manager.switch_branch(attachment, branch_name)
    }

    pub fn get_commits(&self, attachment: &GitRepoAttachment, limit: usize) -> Result<Vec<CommitInfo>> {
        let branch_manager = BranchManager::new();
        branch_manager.get_commits(attachment, limit)
    }

    pub fn get_commit_history(&self, attachment: &GitRepoAttachment, limit: Option<usize>) -> Result<Vec<CommitInfo>> {
        let branch_manager = BranchManager::new();
        branch_manager.get_commits(attachment, limit.unwrap_or(50))
    }

    // Diff operations
    pub fn get_diff(&self, attachment: &GitRepoAttachment, commit_id: &str) -> Result<DiffInfo> {
        let diff_parser = DiffParser::new();
        diff_parser.get_commit_diff(attachment, commit_id)
    }

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
