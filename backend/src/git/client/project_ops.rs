// src/git/client/project_ops.rs
// Project-aware git operations - simplified, no wrapper bullshit

use async_trait::async_trait;

use crate::{
    api::error::{ApiError, ApiResult},
    git::{
        GitClient, GitRepoAttachment,
        client::{BranchInfo, CommitInfo, DiffInfo, FileNode},
    },
};

/// Extension trait that adds project-aware operations to GitClient
/// Each operation looks up the attachment and calls the underlying git method directly
#[async_trait]
pub trait ProjectOps {
    // Project-aware methods that handle attachment lookup internally
    async fn clone_project(&self, project_id: &str) -> ApiResult<GitRepoAttachment>;
    async fn import_project(&self, project_id: &str) -> ApiResult<()>;
    async fn sync_project(&self, project_id: &str, message: &str) -> ApiResult<()>;
    async fn pull_project(&self, project_id: &str) -> ApiResult<()>;
    async fn reset_project(&self, project_id: &str) -> ApiResult<()>;
    async fn get_project_tree(&self, project_id: &str) -> ApiResult<Vec<FileNode>>;
    async fn get_project_file(&self, project_id: &str, path: &str) -> ApiResult<String>;
    async fn update_project_file(
        &self,
        project_id: &str,
        path: &str,
        content: &str,
        message: &str,
    ) -> ApiResult<()>;
    async fn get_project_branches(&self, project_id: &str) -> ApiResult<(Vec<BranchInfo>, String)>;
    async fn switch_project_branch(&self, project_id: &str, branch: &str) -> ApiResult<()>;
    async fn get_project_commits(
        &self,
        project_id: &str,
        limit: usize,
    ) -> ApiResult<Vec<CommitInfo>>;
    async fn get_project_diff(
        &self,
        project_id: &str,
        from: Option<&str>,
        to: Option<&str>,
    ) -> ApiResult<DiffInfo>;
    async fn get_project_file_at_commit(
        &self,
        project_id: &str,
        path: &str,
        commit: &str,
    ) -> ApiResult<String>;
}

#[async_trait]
impl ProjectOps for GitClient {
    async fn clone_project(&self, project_id: &str) -> ApiResult<GitRepoAttachment> {
        let attachment = get_attachment(self, project_id).await?;

        self.clone_repo(&attachment)
            .await
            .map_err(|e| ApiError::internal(format!("Clone failed: {}", e)))?;

        Ok(attachment)
    }

    async fn import_project(&self, project_id: &str) -> ApiResult<()> {
        let attachment = get_attachment(self, project_id).await?;

        self.import_codebase(&attachment)
            .await
            .map_err(|e| ApiError::internal(format!("Import failed: {}", e)))
    }

    async fn sync_project(&self, project_id: &str, message: &str) -> ApiResult<()> {
        let attachment = get_attachment(self, project_id).await?;

        self.sync_changes(&attachment, message)
            .await
            .map_err(|e| ApiError::internal(format!("Sync failed: {}", e)))
    }

    async fn pull_project(&self, project_id: &str) -> ApiResult<()> {
        let attachments = self
            .store
            .list_project_attachments(project_id)
            .await
            .map_err(|e| ApiError::internal(format!("Failed to list attachments: {}", e)))?;

        let attachment = attachments
            .first()
            .ok_or_else(|| ApiError::not_found("No repository attached"))?;

        self.pull_changes(&attachment.id).await
    }

    async fn reset_project(&self, project_id: &str) -> ApiResult<()> {
        let attachments = self
            .store
            .list_project_attachments(project_id)
            .await
            .map_err(|e| ApiError::internal(format!("Failed to list attachments: {}", e)))?;

        let attachment = attachments
            .first()
            .ok_or_else(|| ApiError::not_found("No repository attached"))?;

        self.reset_to_remote(&attachment.id).await
    }

    async fn get_project_tree(&self, project_id: &str) -> ApiResult<Vec<FileNode>> {
        let attachment = get_attachment(self, project_id).await?;

        self.get_file_tree(&attachment)
            .map_err(|e| ApiError::internal(format!("Get tree failed: {}", e)))
    }

    async fn get_project_file(&self, project_id: &str, path: &str) -> ApiResult<String> {
        let attachment = get_attachment(self, project_id).await?;

        self.get_file_content(&attachment, path)
            .map_err(|e| ApiError::internal(format!("Get file failed: {}", e)))
    }

    async fn update_project_file(
        &self,
        project_id: &str,
        path: &str,
        content: &str,
        message: &str,
    ) -> ApiResult<()> {
        let attachment = get_attachment(self, project_id).await?;

        self.update_file_content(&attachment, path, content, Some(message))
            .map_err(|e| ApiError::internal(format!("Update file failed: {}", e)))
    }

    async fn get_project_branches(&self, project_id: &str) -> ApiResult<(Vec<BranchInfo>, String)> {
        let attachment = get_attachment(self, project_id).await?;

        let branches = self
            .get_branches(&attachment)
            .map_err(|e| ApiError::internal(format!("Get branches failed: {}", e)))?;

        // Find current branch using is_head field
        let current = branches
            .iter()
            .find(|b| b.is_head)
            .map(|b| b.name.clone())
            .unwrap_or_else(|| "main".to_string());

        Ok((branches, current))
    }

    async fn switch_project_branch(&self, project_id: &str, branch: &str) -> ApiResult<()> {
        let attachment = get_attachment(self, project_id).await?;

        self.switch_branch(&attachment, branch)
            .map_err(|e| ApiError::internal(format!("Switch branch failed: {}", e)))
    }

    async fn get_project_commits(
        &self,
        project_id: &str,
        limit: usize,
    ) -> ApiResult<Vec<CommitInfo>> {
        let attachment = get_attachment(self, project_id).await?;

        self.get_commits(&attachment, limit)
            .map_err(|e| ApiError::internal(format!("Get commits failed: {}", e)))
    }

    async fn get_project_diff(
        &self,
        project_id: &str,
        _from: Option<&str>,
        to: Option<&str>,
    ) -> ApiResult<DiffInfo> {
        let attachment = get_attachment(self, project_id).await?;
        let commit_id = to.unwrap_or("HEAD");

        self.get_diff(&attachment, commit_id)
            .map_err(|e| ApiError::internal(format!("Get diff failed: {}", e)))
    }

    async fn get_project_file_at_commit(
        &self,
        project_id: &str,
        path: &str,
        commit: &str,
    ) -> ApiResult<String> {
        let attachment = get_attachment(self, project_id).await?;

        self.get_file_at_commit(&attachment, path, commit)
            .map_err(|e| ApiError::internal(format!("Get file at commit failed: {}", e)))
    }
}

/// Helper: Get the first attachment for a project
/// Extracted to avoid duplication across all operations
async fn get_attachment(client: &GitClient, project_id: &str) -> ApiResult<GitRepoAttachment> {
    let attachments = client
        .store
        .list_project_attachments(project_id)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to list attachments: {}", e)))?;

    attachments
        .into_iter()
        .next()
        .ok_or_else(|| ApiError::not_found("No repository attached to this project"))
}
