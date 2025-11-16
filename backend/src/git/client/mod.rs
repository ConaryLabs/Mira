// src/git/client/mod.rs
use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};

use crate::api::error::{ApiError, ApiResult};
use crate::git::store::GitStore;
use crate::git::types::GitRepoAttachment;
use crate::memory::features::code_intelligence::CodeIntelligenceService;

pub mod branch_manager;
pub mod code_sync;
pub mod diff_parser;
pub mod operations;
pub mod project_ops;
pub mod tree_builder;

pub use branch_manager::{BranchInfo, BranchManager, CommitInfo};
pub use code_sync::CodeSync;
pub use diff_parser::{DiffInfo, DiffParser};
pub use operations::GitOperations;
pub use project_ops::ProjectOps;
pub use tree_builder::{FileNode, FileNodeType, TreeBuilder};

#[derive(Clone)]
pub struct GitClient {
    pub git_dir: PathBuf,
    pub store: GitStore,
    pub code_intelligence: Option<CodeIntelligenceService>,
    operations: GitOperations, // FIXED: Store operations instead of creating on every call
}

impl GitClient {
    pub fn new<P: AsRef<Path>>(git_dir: P, store: GitStore) -> Self {
        fs::create_dir_all(&git_dir).ok();

        let git_dir_buf = git_dir.as_ref().to_path_buf();
        let operations = GitOperations::new(git_dir_buf.clone(), store.clone());

        Self {
            git_dir: git_dir_buf,
            store,
            code_intelligence: None,
            operations,
        }
    }

    pub fn with_code_intelligence<P: AsRef<Path>>(
        git_dir: P,
        store: GitStore,
        code_intelligence: CodeIntelligenceService,
    ) -> Self {
        fs::create_dir_all(&git_dir).ok();

        let git_dir_buf = git_dir.as_ref().to_path_buf();

        // Create operations with code sync
        let code_sync = CodeSync::new(store.clone(), code_intelligence.clone());
        let operations =
            GitOperations::with_code_sync(git_dir_buf.clone(), store.clone(), code_sync);

        Self {
            git_dir: git_dir_buf,
            store,
            code_intelligence: Some(code_intelligence),
            operations,
        }
    }

    pub fn has_code_intelligence(&self) -> bool {
        self.code_intelligence.is_some()
    }

    // Core repository operations - FIXED: Use stored operations
    pub async fn attach_repo(&self, project_id: &str, repo_url: &str) -> Result<GitRepoAttachment> {
        self.operations.attach_repo(project_id, repo_url).await
    }

    pub async fn clone_repo(&self, attachment: &GitRepoAttachment) -> Result<()> {
        self.operations.clone_repo(attachment).await
    }

    pub async fn import_codebase(&self, attachment: &GitRepoAttachment) -> Result<()> {
        self.operations.import_codebase(attachment).await
    }

    pub async fn sync_changes(
        &self,
        attachment: &GitRepoAttachment,
        commit_message: &str,
    ) -> Result<()> {
        self.operations
            .sync_changes(attachment, commit_message)
            .await
    }

    pub async fn pull_changes(&self, attachment_id: &str) -> ApiResult<()> {
        // Fetch the attachment from store first
        let attachment = self
            .store
            .get_attachment(attachment_id)
            .await
            .map_err(|e| ApiError::internal(format!("Failed to get git attachment: {}", e)))?
            .ok_or_else(|| {
                ApiError::not_found(format!("Git attachment not found: {}", attachment_id))
            })?;

        self.operations
            .pull_changes(&attachment)
            .await
            .map_err(|e| ApiError::internal(format!("Failed to pull changes: {}", e)))
    }

    pub async fn reset_to_remote(&self, attachment_id: &str) -> ApiResult<()> {
        self.operations.reset_to_remote(attachment_id).await
    }

    // File operations - use stored operations
    pub fn get_file_tree(&self, attachment: &GitRepoAttachment) -> Result<Vec<FileNode>> {
        let builder = tree_builder::TreeBuilder::new();
        builder.get_file_tree(attachment)
    }

    pub fn get_file_content(&self, attachment: &GitRepoAttachment, path: &str) -> Result<String> {
        let full_path = Path::new(&attachment.local_path).join(path);
        std::fs::read_to_string(&full_path)
            .map_err(|e| anyhow::anyhow!("Failed to read file {}: {}", path, e))
    }

    pub fn update_file_content(
        &self,
        attachment: &GitRepoAttachment,
        path: &str,
        content: &str,
        _message: Option<&str>,
    ) -> Result<()> {
        let full_path = Path::new(&attachment.local_path).join(path);

        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        std::fs::write(&full_path, content)
            .map_err(|e| anyhow::anyhow!("Failed to write file {}: {}", path, e))
    }

    // Branch operations
    pub fn get_branches(&self, attachment: &GitRepoAttachment) -> Result<Vec<BranchInfo>> {
        let manager = branch_manager::BranchManager::new();
        manager.get_branches(attachment)
    }

    pub fn switch_branch(&self, attachment: &GitRepoAttachment, branch: &str) -> Result<()> {
        let manager = branch_manager::BranchManager::new();
        manager.switch_branch(attachment, branch)
    }

    pub fn get_commits(
        &self,
        attachment: &GitRepoAttachment,
        limit: usize,
    ) -> Result<Vec<CommitInfo>> {
        let manager = branch_manager::BranchManager::new();
        manager.get_commits(attachment, limit)
    }

    // Diff operations
    pub fn get_diff(&self, attachment: &GitRepoAttachment, commit_id: &str) -> Result<DiffInfo> {
        let parser = diff_parser::DiffParser::new();
        parser.get_commit_diff(attachment, commit_id)
    }

    pub fn get_file_at_commit(
        &self,
        attachment: &GitRepoAttachment,
        path: &str,
        commit: &str,
    ) -> Result<String> {
        let parser = diff_parser::DiffParser::new();
        parser.get_file_at_commit(attachment, path, commit)
    }
}
