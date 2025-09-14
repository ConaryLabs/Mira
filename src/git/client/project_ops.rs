// src/git/client/project_ops.rs
// Project-aware operations that handle attachment lookup internally

use anyhow::Result;
use std::sync::Arc;
use async_trait::async_trait;

use crate::{
    api::error::{ApiError, ApiResult},
    git::{
        GitClient, 
        GitRepoAttachment,
        client::{FileNode, BranchInfo, CommitInfo, DiffInfo},
    },
};

/// Extension trait that adds project-aware operations to GitClient
/// Handles all the repetitive attachment lookup bullshit internally
#[async_trait]
pub trait ProjectOps {
    /// Execute an operation with the project's attachment
    async fn with_project_attachment<F, R>(&self, project_id: &str, op: F) -> ApiResult<R>
    where
        F: FnOnce(&GitRepoAttachment, &GitClient) -> Result<R> + Send,
        R: Send;
    
    /// Execute an async operation with the project's attachment  
    async fn with_project_attachment_async<F, Fut, R>(&self, project_id: &str, op: F) -> ApiResult<R>
    where
        F: FnOnce(GitRepoAttachment, Arc<GitClient>) -> Fut + Send,
        Fut: std::future::Future<Output = Result<R>> + Send,
        R: Send;
    
    // Project-aware methods that handle attachment lookup internally
    async fn clone_project(&self, project_id: &str) -> ApiResult<GitRepoAttachment>;
    async fn import_project(&self, project_id: &str) -> ApiResult<()>;
    async fn sync_project(&self, project_id: &str, message: &str) -> ApiResult<()>;
    async fn pull_project(&self, project_id: &str) -> ApiResult<()>;
    async fn reset_project(&self, project_id: &str) -> ApiResult<()>;
    async fn get_project_tree(&self, project_id: &str) -> ApiResult<Vec<FileNode>>;
    async fn get_project_file(&self, project_id: &str, path: &str) -> ApiResult<String>;
    async fn update_project_file(&self, project_id: &str, path: &str, content: &str, message: &str) -> ApiResult<()>;
    async fn get_project_branches(&self, project_id: &str) -> ApiResult<(Vec<BranchInfo>, String)>;
    async fn switch_project_branch(&self, project_id: &str, branch: &str) -> ApiResult<()>;
    async fn get_project_commits(&self, project_id: &str, limit: usize) -> ApiResult<Vec<CommitInfo>>;
    async fn get_project_diff(&self, project_id: &str, from: Option<&str>, to: Option<&str>) -> ApiResult<DiffInfo>;
    async fn get_project_file_at_commit(&self, project_id: &str, path: &str, commit: &str) -> ApiResult<String>;
}

#[async_trait]
impl ProjectOps for GitClient {
    async fn with_project_attachment<F, R>(&self, project_id: &str, op: F) -> ApiResult<R>
    where
        F: FnOnce(&GitRepoAttachment, &GitClient) -> Result<R> + Send,
        R: Send,
    {
        let attachments = self.store
            .list_project_attachments(project_id)
            .await
            .map_err(|e| ApiError::internal(format!("Failed to list attachments: {}", e)))?;
        
        let attachment = attachments
            .first()
            .ok_or_else(|| ApiError::not_found("No repository attached to this project"))?;
        
        op(attachment, self)
            .map_err(|e| ApiError::internal(format!("Operation failed: {}", e)))
    }
    
    async fn with_project_attachment_async<F, Fut, R>(&self, project_id: &str, op: F) -> ApiResult<R>
    where
        F: FnOnce(GitRepoAttachment, Arc<GitClient>) -> Fut + Send,
        Fut: std::future::Future<Output = Result<R>> + Send,
        R: Send,
    {
        let attachments = self.store
            .list_project_attachments(project_id)
            .await
            .map_err(|e| ApiError::internal(format!("Failed to list attachments: {}", e)))?;
        
        let attachment = attachments
            .into_iter()
            .next()
            .ok_or_else(|| ApiError::not_found("No repository attached to this project"))?;
        
        let client = Arc::new(self.clone());
        op(attachment, client)
            .await
            .map_err(|e| ApiError::internal(format!("Operation failed: {}", e)))
    }
    
    async fn clone_project(&self, project_id: &str) -> ApiResult<GitRepoAttachment> {
        self.with_project_attachment_async(project_id, |attachment, client| async move {
            client.clone_repo(&attachment).await?;
            Ok(attachment)
        }).await
    }
    
    async fn import_project(&self, project_id: &str) -> ApiResult<()> {
        self.with_project_attachment_async(project_id, |attachment, client| async move {
            client.import_codebase(&attachment).await
        }).await
    }
    
    async fn sync_project(&self, project_id: &str, message: &str) -> ApiResult<()> {
        let msg = message.to_string();
        self.with_project_attachment_async(project_id, |attachment, client| async move {
            client.sync_changes(&attachment, &msg).await
        }).await
    }
    
    async fn pull_project(&self, project_id: &str) -> ApiResult<()> {
        // pull_changes takes attachment_id, not attachment
        let attachments = self.store.list_project_attachments(project_id).await
            .map_err(|e| ApiError::internal(format!("Failed to list attachments: {}", e)))?;
        
        if let Some(attachment) = attachments.first() {
            self.pull_changes(&attachment.id).await
        } else {
            Err(ApiError::not_found("No repository attached"))
        }
    }
    
    async fn reset_project(&self, project_id: &str) -> ApiResult<()> {
        // Same as pull - reset_to_remote takes attachment_id
        let attachments = self.store.list_project_attachments(project_id).await
            .map_err(|e| ApiError::internal(format!("Failed to list attachments: {}", e)))?;
        
        if let Some(attachment) = attachments.first() {
            self.reset_to_remote(&attachment.id).await
        } else {
            Err(ApiError::not_found("No repository attached"))
        }
    }
    
    async fn get_project_tree(&self, project_id: &str) -> ApiResult<Vec<FileNode>> {
        self.with_project_attachment(project_id, |attachment, client| {
            client.get_file_tree(attachment)
        }).await
    }
    
    async fn get_project_file(&self, project_id: &str, path: &str) -> ApiResult<String> {
        let path_owned = path.to_string();
        self.with_project_attachment(project_id, move |attachment, client| {
            client.get_file_content(attachment, &path_owned)
        }).await
    }
    
    async fn update_project_file(&self, project_id: &str, path: &str, content: &str, message: &str) -> ApiResult<()> {
        let path_owned = path.to_string();
        let content_owned = content.to_string();
        let message_owned = Some(message);  // FIX: wrap in Option
        self.with_project_attachment(project_id, move |attachment, client| {
            client.update_file_content(attachment, &path_owned, &content_owned, message_owned)
        }).await
    }
    
    async fn get_project_branches(&self, project_id: &str) -> ApiResult<(Vec<BranchInfo>, String)> {
        self.with_project_attachment(project_id, |attachment, client| {
            let branches = client.get_branches(attachment)?;
            // FIX: use is_head instead of is_current
            let current = branches.iter()
                .find(|b| b.is_head)
                .map(|b| b.name.clone())
                .unwrap_or_else(|| "main".to_string());
            Ok((branches, current))
        }).await
    }
    
    async fn switch_project_branch(&self, project_id: &str, branch: &str) -> ApiResult<()> {
        let branch_owned = branch.to_string();
        self.with_project_attachment(project_id, move |attachment, client| {
            client.switch_branch(attachment, &branch_owned)
        }).await
    }
    
    async fn get_project_commits(&self, project_id: &str, limit: usize) -> ApiResult<Vec<CommitInfo>> {
        self.with_project_attachment(project_id, move |attachment, client| {
            client.get_commits(attachment, limit)
        }).await
    }
    
    async fn get_project_diff(&self, project_id: &str, _from: Option<&str>, to: Option<&str>) -> ApiResult<DiffInfo> {
        let commit_id = to.unwrap_or("HEAD").to_string();
        self.with_project_attachment(project_id, move |attachment, client| {
            client.get_diff(attachment, &commit_id)
        }).await
    }
    
    async fn get_project_file_at_commit(&self, project_id: &str, path: &str, commit: &str) -> ApiResult<String> {
        let path_owned = path.to_string();
        let commit_owned = commit.to_string();
        self.with_project_attachment(project_id, move |attachment, client| {
            client.get_file_at_commit(attachment, &path_owned, &commit_owned)
        }).await
    }
}
