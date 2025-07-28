use anyhow::{Result, Context};
use git2::Repository;
use std::fs;
use std::path::{Path, PathBuf};
use chrono::Utc;
use uuid::Uuid;

use super::types::{GitRepoAttachment, GitImportStatus};
use super::store::GitStore;

/// Main API for attaching, cloning, importing, and syncing a GitHub repo for a project.
#[derive(Clone)]
pub struct GitClient {
    pub git_dir: PathBuf,
    pub store: GitStore,
}

impl GitClient {
    /// Create a new client with a directory for all clones (e.g. "./repos")
    pub fn new<P: AsRef<Path>>(git_dir: P, store: GitStore) -> Self {
        fs::create_dir_all(&git_dir).ok();
        Self {
            git_dir: git_dir.as_ref().to_path_buf(),
            store,
        }
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
            .unwrap()
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

        self.store.create_attachment(&attachment).await?;
        Ok(attachment)
    }

    /// Clone the attached repo to disk. Returns Result<()>.
    pub async fn clone_repo(&self, attachment: &GitRepoAttachment) -> Result<()> {
        // For MVP/public repos: no custom fetch options or callbacks needed.
        // (Add auth/ssh in a future sprint.)
        Repository::clone(
            &attachment.repo_url,
            &attachment.local_path,
        )
        .context("Failed to clone repository")?;

        self.store.update_import_status(&attachment.id, GitImportStatus::Cloned).await?;

        Ok(())
    }

    /// Import files into your DB (MVP: just record file paths and contents)
    pub async fn import_codebase(&self, attachment: &GitRepoAttachment) -> Result<()> {
        let repo_path = Path::new(&attachment.local_path);
        let mut _files_imported = 0;

        for entry in walkdir::WalkDir::new(repo_path)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
        {
            let _rel_path = entry
                .path()
                .strip_prefix(repo_path)
                .unwrap()
                .to_str()
                .unwrap()
                .to_string();
            let _contents = fs::read_to_string(entry.path()).unwrap_or_default();

            // You would write this to your codebase DB here:
            // e.g., CodeFile { attachment_id, rel_path, contents }

            _files_imported += 1;
        }

        self.store.update_import_status(&attachment.id, GitImportStatus::Imported).await?;
        self.store.update_last_sync(&attachment.id, Utc::now()).await?;

        Ok(())
    }

    /// Sync: commit and push DB-side code changes back to GitHub.
    pub async fn sync_changes(&self, attachment: &GitRepoAttachment, commit_message: &str) -> Result<()> {
        // Do all git operations in a block to ensure git2 types are dropped before async operations
        {
            let repo = Repository::open(&attachment.local_path)
                .context("Could not open local repo for syncing")?;

            let mut index = repo.index()?;
            index.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)?;
            index.write()?;

            let oid = index.write_tree()?;
            let signature = repo.signature()?;
            let parent_commit = repo.head()?.peel_to_commit()?;
            let tree = repo.find_tree(oid)?;

            repo.commit(
                Some("HEAD"),
                &signature,
                &signature,
                commit_message,
                &tree,
                &[&parent_commit],
            )?;

            let mut remote = repo.find_remote("origin")?;
            remote.push(&["refs/heads/main:refs/heads/main"], None)
                .context("Failed to push to remote")?;
            
            // Drop all git2 types before async operations
        }

        // Now do async operations after git2 types are dropped
        self.store.update_last_sync(&attachment.id, Utc::now()).await?;
        Ok(())
    }
}
