// src/git/client/operations.rs
// Core git repository operations: attach, clone, import, sync
// Enhanced with code intelligence integration for AST analysis

use anyhow::Result;
use chrono::Utc;
use git2::Repository;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{info, debug, warn};

use crate::git::types::{GitRepoAttachment, GitImportStatus};
use crate::git::store::GitStore;
use crate::api::error::{IntoApiError, ApiResult};
use crate::memory::features::code_intelligence::CodeIntelligenceService;

/// Handles core Git repository operations with optional code intelligence
#[derive(Clone)]
pub struct GitOperations {
    git_dir: PathBuf,
    store: GitStore,
    code_intelligence: Option<CodeIntelligenceService>,
}

impl GitOperations {
    /// Create new git operations handler
    pub fn new(git_dir: PathBuf, store: GitStore) -> Self {
        Self { 
            git_dir, 
            store,
            code_intelligence: None,
        }
    }

    /// Create new git operations handler with code intelligence
    pub fn with_code_intelligence(
        git_dir: PathBuf, 
        store: GitStore, 
        code_intelligence: CodeIntelligenceService
    ) -> Self {
        Self { 
            git_dir, 
            store,
            code_intelligence: Some(code_intelligence),
        }
    }

    /// Attach a repo: generate an ID, determine clone path, and persist the attachment
    pub async fn attach_repo(
        &self,
        project_id: &str,
        repo_url: &str,
    ) -> Result<GitRepoAttachment> {
        use uuid::Uuid;
        
        let id = Uuid::new_v4().to_string();
        let local_path = self.git_dir.join(&id).to_string_lossy().to_string();

        let attachment = GitRepoAttachment {
            id,
            project_id: project_id.to_string(),
            repo_url: repo_url.to_string(),
            local_path,
            import_status: GitImportStatus::Pending,
            last_imported_at: None,
            last_sync_at: None,
        };

        self.store.create_attachment(&attachment).await
            .into_api_error("Failed to create git attachment")?;

        info!("Attached repository {} for project {}", repo_url, project_id);
        Ok(attachment)
    }

    /// Clone the attached repo to disk
    pub async fn clone_repo(&self, attachment: &GitRepoAttachment) -> Result<()> {
        info!("Cloning repository {} to {}", attachment.repo_url, attachment.local_path);

        if let Some(parent) = Path::new(&attachment.local_path).parent() {
            fs::create_dir_all(parent)
                .into_api_error("Failed to create repository directory")?;
        }

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

    /// Import files into your DB with optional code intelligence analysis
    pub async fn import_codebase(&self, attachment: &GitRepoAttachment) -> Result<()> {
        info!("Importing codebase for repository {}", attachment.id);

        let repo_path = Path::new(&attachment.local_path);
        if !repo_path.exists() {
            return Err(anyhow::anyhow!("Repository not found at {}", attachment.local_path));
        }

        let repo_path = repo_path.to_path_buf();
        let files = tokio::task::spawn_blocking(move || {
            walk_directory(&repo_path)
        })
        .await
        .into_api_error("Failed to spawn blocking task")?
        .into_api_error("Failed to walk repository directory")?;
        
        debug!("Found {} files to import", files.len());

        // Code intelligence analysis if enabled
        if let Some(ref code_intel) = self.code_intelligence {
            let mut analyzed_files = 0;
            let mut analysis_errors = 0;

            for file_path in &files {
                if !is_rust_file(file_path) {
                    continue;
                }

                match self.analyze_file(code_intel, file_path, &attachment.id).await {
                    Ok(()) => {
                        analyzed_files += 1;
                        debug!("Successfully analyzed: {}", file_path.display());
                    }
                    Err(e) => {
                        analysis_errors += 1;
                        warn!("Failed to analyze {}: {} (continuing import)", file_path.display(), e);
                    }
                }
            }

            info!(
                "Code intelligence analysis complete: {} files analyzed, {} errors",
                analyzed_files, analysis_errors
            );
        } else {
            debug!("Code intelligence not enabled for this import");
        }

        self.store.update_import_status(&attachment.id, GitImportStatus::Imported)
            .await
            .into_api_error("Failed to update import status")?;

        self.store.update_last_sync(&attachment.id, Utc::now())
            .await
            .into_api_error("Failed to update last sync time")?;

        info!("Successfully imported {} files for repository {}", files.len(), attachment.id);
        Ok(())
    }

    /// Analyze a single file with code intelligence
    async fn analyze_file(
        &self,
        code_intel: &CodeIntelligenceService,
        file_path: &Path,
        attachment_id: &str,
    ) -> Result<()> {
        let content = tokio::fs::read_to_string(file_path).await
            .map_err(|e| anyhow::anyhow!("Failed to read file {}: {}", file_path.display(), e))?;

        let file_id = calculate_file_id(file_path, attachment_id);
        let file_path_str = file_path.to_string_lossy();
        
        let result = code_intel.analyze_and_store_file(
            file_id,
            &content,
            &file_path_str,
            "rust",
        ).await?;

        debug!(
            "Analyzed {}: {} elements, complexity {}, {} quality issues",
            file_path_str,
            result.elements_count,
            result.complexity_score,
            result.quality_issues_count
        );

        Ok(())
    }

    /// Sync: commit and push DB-side code changes back to GitHub
    pub async fn sync_changes(&self, attachment: &GitRepoAttachment, commit_message: &str) -> Result<()> {
        info!("Syncing changes for repository {}", attachment.id);

        let local_path = attachment.local_path.clone();
        let commit_msg = commit_message.to_string();
        
        tokio::task::spawn_blocking(move || {
            let repo = Repository::open(&local_path)
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

        self.store.update_last_sync(&attachment.id, Utc::now())
            .await
            .into_api_error("Failed to update last sync time")?;

        info!("Successfully synced changes for repository {}", attachment.id);
        Ok(())
    }

    /// Pull latest changes from remote
    pub async fn pull_changes(&self, attachment_id: &str) -> ApiResult<()> {
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

/// Check if a file is a Rust source file
fn is_rust_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("rs"))
        .unwrap_or(false)
}

/// Calculate a unique file ID based on path and attachment
fn calculate_file_id(file_path: &Path, attachment_id: &str) -> i64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    
    let mut hasher = DefaultHasher::new();
    attachment_id.hash(&mut hasher);
    file_path.to_string_lossy().hash(&mut hasher);
    
    (hasher.finish() as i64).abs()
}

/// Walk directory and collect files
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
    
    if path_str.contains("/.git/") || path_str.starts_with(".git/") {
        return true;
    }

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
