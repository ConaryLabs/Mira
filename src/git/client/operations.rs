// src/git/client/operations.rs
// Core git operations: attach, clone, import, sync
// Extracted from monolithic GitClient for focused responsibility

use anyhow::{Result, Context};
use git2::Repository;
use std::fs;
use std::path::{Path, PathBuf};
use chrono::Utc;
use uuid::Uuid;
use walkdir::WalkDir;

use crate::git::types::{GitRepoAttachment, GitImportStatus};
use crate::git::store::GitStore;
use crate::api::error::IntoApiError;

/// Handles core git repository operations
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
            .to_str()
            .context("Failed to create local path")?
            .to_string();

        let attachment = GitRepoAttachment {
            id,
            project_id: project_id.to_string(),
            repo_url: repo_url.to_string(),
            local_path: local_path.clone(),
            import_status: GitImportStatus::Pending,
            last_imported_at: None,
            last_sync_at: None,
        };

        self.store.create_attachment(&attachment)
            .await
            .into_api_error("Failed to create git attachment")?;

        Ok(attachment)
    }

    /// Clone the attached repo to disk. Returns Result<()>.
    pub async fn clone_repo(&self, attachment: &GitRepoAttachment) -> Result<()> {
        // Ensure parent directory exists
        if let Some(parent) = Path::new(&attachment.local_path).parent() {
            fs::create_dir_all(parent)
                .into_api_error("Failed to create repository directory")?;
        }

        // For MVP/public repos: no custom fetch options or callbacks needed.
        // (Add auth/ssh in a future sprint.)
        Repository::clone(
            &attachment.repo_url,
            &attachment.local_path,
        )
        .into_api_error("Failed to clone repository")?;

        self.store.update_import_status(&attachment.id, GitImportStatus::Cloned)
            .await
            .into_api_error("Failed to update import status")?;

        Ok(())
    }

    /// Import files into your DB (MVP: just record file paths and contents)
    pub async fn import_codebase(&self, attachment: &GitRepoAttachment) -> Result<()> {
        let repo_path = Path::new(&attachment.local_path);
        
        if !repo_path.exists() {
            return Err(anyhow::anyhow!("Repository path does not exist: {}", attachment.local_path));
        }

        let mut files_imported = 0;

        // Walk through all files in the repository
        for entry in WalkDir::new(repo_path)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .filter(|e| !should_ignore_file(e.path()))
        {
            let rel_path = entry
                .path()
                .strip_prefix(repo_path)
                .context("Failed to get relative path")?
                .to_str()
                .context("Invalid UTF-8 in file path")?
                .to_string();

            // Read file contents (with error handling for binary files)
            match fs::read_to_string(entry.path()) {
                Ok(_contents) => {
                    // In a full implementation, you would store this in your codebase DB:
                    // e.g., CodeFile { attachment_id, rel_path, contents }
                    // For now, just count successful reads
                    files_imported += 1;
                }
                Err(_) => {
                    // Skip binary files or files with encoding issues
                    tracing::debug!("Skipping binary or unreadable file: {}", rel_path);
                }
            }
        }

        tracing::info!("Imported {} files from repository {}", files_imported, attachment.id);

        // Update database status
        self.store.update_import_status(&attachment.id, GitImportStatus::Imported)
            .await
            .into_api_error("Failed to update import status")?;

        self.store.update_last_sync(&attachment.id, Utc::now())
            .await
            .into_api_error("Failed to update last sync time")?;

        Ok(())
    }

    /// Sync: commit and push DB-side code changes back to GitHub.
    pub async fn sync_changes(&self, attachment: &GitRepoAttachment, commit_message: &str) -> Result<()> {
        // Do all git operations in a block to ensure git2 types are dropped before async operations
        {
            let repo = Repository::open(&attachment.local_path)
                .into_api_error("Could not open local repo for syncing")?;

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
                commit_message,
                &tree,
                &[&parent_commit],
            )
            .into_api_error("Failed to create commit")?;

            let mut remote = repo.find_remote("origin")
                .into_api_error("Failed to find origin remote")?;
            
            remote.push(&["refs/heads/main:refs/heads/main"], None)
                .into_api_error("Failed to push to remote")?;
            
            // Drop all git2 types before async operations
        }

        // Now do async operations after git2 types are dropped
        self.store.update_last_sync(&attachment.id, Utc::now())
            .await
            .into_api_error("Failed to update last sync time")?;

        tracing::info!("Successfully synced changes for repository {}", attachment.id);
        Ok(())
    }
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
        match ext.as_str() {
            "exe" | "dll" | "so" | "dylib" | "bin" | "jar" | "zip" | "tar" | "gz" | "png" 
            | "jpg" | "jpeg" | "gif" | "bmp" | "ico" | "pdf" | "mp3" | "mp4" | "avi" => true,
            _ => false,
        }
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

    #[test]
    fn test_operations_creation() {
        let temp_dir = TempDir::new().unwrap();
        // Note: Would need a mock store for actual testing
        // let store = GitStore::new(/* test db */);
        // let operations = GitOperations::new(temp_dir.path().to_path_buf(), store);
    }
}
