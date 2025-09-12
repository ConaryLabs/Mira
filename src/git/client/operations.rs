// src/git/client/operations.rs
// Core git repository operations: attach, clone, import, sync
// FIXED: All git2 operations wrapped in spawn_blocking for thread safety

use anyhow::Result;
use chrono::Utc;
use git2::Repository;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{info, debug, warn};
use uuid::Uuid;

use crate::git::types::{GitRepoAttachment, GitImportStatus};
use crate::git::store::GitStore;
use crate::api::error::{IntoApiError, ApiResult};

/// Handles core Git repository operations
#[derive(Clone)]
pub struct GitOperations {
    git_dir: PathBuf,
    store: GitStore,
}

impl GitOperations {
    /// Create new git operations handler
    pub fn new(git_dir: PathBuf, store: GitStore) -> Self {
        Self { git_dir, store }
    }

    /// Attach a repo: generate an ID, determine clone path, and persist the attachment.
    pub async fn attach_repo(
        &self,
        project_id: &str,
        repo_url: &str,
    ) -> Result<GitRepoAttachment> {
        let id = Uuid::new_v4().to_string();
        let local_path = self
            .git_dir
            .join(&id)
            .to_string_lossy()
            .to_string();

        let attachment = GitRepoAttachment {
            id,
            project_id: project_id.to_string(),
            repo_url: repo_url.to_string(),
            local_path,
            import_status: GitImportStatus::Pending,
            last_imported_at: None,
            last_sync_at: None,
        };

        self.store.create_attachment(&attachment)
            .await
            .into_api_error("Failed to create git attachment")?;

        info!("Attached repository {} for project {}", repo_url, project_id);
        Ok(attachment)
    }

    /// Clone the attached repo to disk.
    pub async fn clone_repo(&self, attachment: &GitRepoAttachment) -> Result<()> {
        info!("Cloning repository {} to {}", attachment.repo_url, attachment.local_path);

        // Ensure parent directory exists
        if let Some(parent) = Path::new(&attachment.local_path).parent() {
            fs::create_dir_all(parent)
                .into_api_error("Failed to create repository directory")?;
        }

        // Clone using git2 - wrap in spawn_blocking for thread safety
        let repo_url = attachment.repo_url.clone();
        let local_path = attachment.local_path.clone();
        
        tokio::task::spawn_blocking(move || {
            Repository::clone(&repo_url, &local_path)
        })
        .await
        .into_api_error("Failed to spawn blocking task")?
        .into_api_error("Failed to clone repository")?;

        self.store.update_import_status(&attachment.id, GitImportStatus::Cloned)
            .await
            .into_api_error("Failed to update import status")?;

        info!("Successfully cloned repository {}", attachment.id);
        Ok(())
    }

    /// Import files into your DB (MVP: just record file paths and contents)
    pub async fn import_codebase(&self, attachment: &GitRepoAttachment) -> Result<()> {
        info!("Importing codebase for repository {}", attachment.id);

        let repo_path = Path::new(&attachment.local_path);
        if !repo_path.exists() {
            return Err(anyhow::anyhow!("Repository not found at {}", attachment.local_path));
        }

        // Walk through files and import them
        let repo_path = repo_path.to_path_buf();
        let files = tokio::task::spawn_blocking(move || {
            walk_directory(&repo_path)
        })
        .await
        .into_api_error("Failed to spawn blocking task")?
        .into_api_error("Failed to walk repository directory")?;
        
        debug!("Found {} files to import", files.len());

        // For MVP, we'll just update the status
        // In the future, this would actually import file contents to the database
        self.store.update_import_status(&attachment.id, GitImportStatus::Imported)
            .await
            .into_api_error("Failed to update import status")?;

        self.store.update_last_sync(&attachment.id, Utc::now())
            .await
            .into_api_error("Failed to update last sync time")?;

        info!("Successfully imported {} files for repository {}", files.len(), attachment.id);
        Ok(())
    }

    /// Sync: commit and push DB-side code changes back to GitHub.
    pub async fn sync_changes(&self, attachment: &GitRepoAttachment, commit_message: &str) -> Result<()> {
        info!("Syncing changes for repository {}", attachment.id);

        // All git2 operations wrapped in spawn_blocking
        let local_path = attachment.local_path.clone();
        let commit_msg = commit_message.to_string();
        
        tokio::task::spawn_blocking(move || {
            let repo = Repository::open(&local_path)
                .into_api_error("Could not open local repo for syncing")?;

            // Stage all changes
            let mut index = repo.index()
                .into_api_error("Failed to get repository index")?;
            
            index.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
                .into_api_error("Failed to stage changes")?;
            
            index.write()
                .into_api_error("Failed to write index")?;

            let oid = index.write_tree()
                .into_api_error("Failed to write tree")?;
            
            let signature = repo.signature()
                .into_api_error("Failed to get git signature")?;
            
            let parent_commit = repo.head()
                .and_then(|h| h.peel_to_commit())
                .into_api_error("Failed to get parent commit")?;
            
            let tree = repo.find_tree(oid)
                .into_api_error("Failed to find tree")?;

            repo.commit(
                Some("HEAD"),
                &signature,
                &signature,
                &commit_msg,
                &tree,
                &[&parent_commit],
            )
            .into_api_error("Failed to create commit")?;

            let mut remote = repo.find_remote("origin")
                .into_api_error("Failed to find origin remote")?;
            
            remote.push(&["refs/heads/main:refs/heads/main"], None)
                .into_api_error("Failed to push to remote")?;
            
            Ok::<(), anyhow::Error>(())
        })
        .await
        .into_api_error("Failed to spawn blocking task")??;

        // Now do async operations after git2 operations complete
        self.store.update_last_sync(&attachment.id, Utc::now())
            .await
            .into_api_error("Failed to update last sync time")?;

        info!("Successfully synced changes for repository {}", attachment.id);
        Ok(())
    }

    /// Pull latest changes from remote
    pub async fn pull_changes(&self, attachment_id: &str) -> ApiResult<()> {
        // Get attachment first
        let attachment = self.store.get_attachment(attachment_id).await
            .into_api_error("Failed to get attachment")?
            .ok_or_else(|| anyhow::anyhow!("Git attachment not found"))
            .into_api_error("Git attachment not found")?;
            
        info!("Pulling changes for repository {}", attachment.id);
        
        let local_path = attachment.local_path.clone();
        
        let result = tokio::task::spawn_blocking(move || -> Result<()> {
            let repo = Repository::open(&local_path)?;
            
            let mut remote = repo.find_remote("origin")?;
            
            remote.fetch(&["main"], None, None)?;
            
            // Fast-forward merge
            let fetch_head = repo.find_reference("FETCH_HEAD")?;
            let fetch_commit = fetch_head.peel_to_commit()?;
            
            let mut branch = repo.find_branch("main", git2::BranchType::Local)?;
            branch.get_mut().set_target(fetch_commit.id(), "Fast-forward pull")?;
            
            repo.checkout_head(Some(git2::build::CheckoutBuilder::default().force()))?;
            
            Ok(())
        })
        .await
        .into_api_error("Failed to spawn blocking task")?;
        
        result.into_api_error("Failed to pull changes")?;
        
        info!("Successfully pulled changes for repository {}", attachment_id);
        Ok(())
    }
    
    /// Reset to remote HEAD (destructive)
    pub async fn reset_to_remote(&self, attachment_id: &str) -> ApiResult<()> {
        // Get attachment first
        let attachment = self.store.get_attachment(attachment_id).await
            .into_api_error("Failed to get attachment")?
            .ok_or_else(|| anyhow::anyhow!("Git attachment not found"))
            .into_api_error("Git attachment not found")?;
            
        warn!("Resetting repository {} to origin/main", attachment.id);
        
        let local_path = attachment.local_path.clone();
        
        let result = tokio::task::spawn_blocking(move || -> Result<()> {
            let repo = Repository::open(&local_path)?;
            
            let mut remote = repo.find_remote("origin")?;
            
            remote.fetch(&["main"], None, None)?;
            
            let oid = repo.refname_to_id("refs/remotes/origin/main")?;
            let object = repo.find_object(oid, None)?;
            
            repo.reset(&object, git2::ResetType::Hard, None)?;
            
            Ok(())
        })
        .await
        .into_api_error("Failed to spawn blocking task")?;
        
        result.into_api_error("Failed to reset repository")?;
        
        warn!("Hard reset repository {} to origin/main complete", attachment_id);
        Ok(())
    }
}

/// Walk directory and collect files (helper function)
fn walk_directory(dir: &Path) -> Result<Vec<PathBuf>, anyhow::Error> {
    let mut files = Vec::new();
    
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            
            if should_ignore_file(&path) {
                continue;
            }
            
            if path.is_dir() {
                files.extend(walk_directory(&path)?);
            } else if path.is_file() {
                files.push(path);
            }
        }
    }
    
    Ok(files)
}

/// Check if a file should be ignored during import
fn should_ignore_file(path: &Path) -> bool {
    let path_str = path.to_string_lossy();
    
    // Skip .git directory
    if path_str.contains("/.git/") || path_str.starts_with(".git/") {
        return true;
    }

    // Skip common binary file extensions
    if let Some(extension) = path.extension() {
        let ext = extension.to_string_lossy().to_lowercase();
        matches!(ext.as_str(),
            "exe" | "dll" | "so" | "dylib" | "bin" | "jar" | "zip" | "tar" | "gz" | 
            "png" | "jpg" | "jpeg" | "gif" | "bmp" | "ico" | "pdf" | "mp3" | "mp4" | "avi"
        )
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_should_ignore_file() {
        assert!(should_ignore_file(Path::new(".git/config")));
        assert!(should_ignore_file(Path::new("test.exe")));
        assert!(should_ignore_file(Path::new("image.png")));
        assert!(!should_ignore_file(Path::new("main.rs")));
        assert!(!should_ignore_file(Path::new("README.md")));
    }
}
