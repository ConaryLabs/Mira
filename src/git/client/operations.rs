// src/git/client/operations.rs

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

#[derive(Clone)]
pub struct GitOperations {
    git_dir: PathBuf,
    store: GitStore,
    code_intelligence: Option<CodeIntelligenceService>,
}

impl GitOperations {
    pub fn new(git_dir: PathBuf, store: GitStore) -> Self {
        Self { 
            git_dir, 
            store,
            code_intelligence: None,
        }
    }

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

    pub async fn attach_repo(&self, project_id: &str, repo_url: &str) -> Result<GitRepoAttachment> {
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

        let mut inserted_files = Vec::new();
        for file_path in &files {
            match self.insert_file_record(file_path, &attachment.id).await {
                Ok(file_id) => {
                    inserted_files.push((file_id, file_path.clone()));
                    debug!("Inserted file record: {} -> file_id {}", file_path.display(), file_id);
                }
                Err(e) => {
                    warn!("Failed to insert file record for {}: {} (skipping)", file_path.display(), e);
                }
            }
        }

        info!("Inserted {} file records into repository_files", inserted_files.len());

        if let Some(ref code_intel) = self.code_intelligence {
            let mut analyzed_files = 0;
            let mut analysis_errors = 0;

            for (file_id, file_path) in &inserted_files {
                if !is_rust_file(file_path) {
                    continue;
                }

                match self.analyze_file_with_id(code_intel, *file_id, file_path).await {
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

    async fn insert_file_record(&self, file_path: &Path, attachment_id: &str) -> Result<i64> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        let content = tokio::fs::read(file_path).await
            .map_err(|e| anyhow::anyhow!("Failed to read file {}: {}", file_path.display(), e))?;
        
        let mut hasher = DefaultHasher::new();
        content.hash(&mut hasher);
        let content_hash = format!("{:x}", hasher.finish());
        
        let content_str = String::from_utf8_lossy(&content);
        let line_count = content_str.lines().count() as i32;
        
        let language = if is_rust_file(file_path) {
            Some("rust".to_string())
        } else {
            None
        };
        
        let repo_path = Path::new(&self.git_dir).join(attachment_id);
        let relative_path = file_path.strip_prefix(&repo_path)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| file_path.to_string_lossy().to_string());
        
        self.store.insert_repository_file(
            attachment_id,
            &relative_path,
            &content_hash,
            language.as_deref(),
            line_count,
        ).await
    }

    async fn analyze_file_with_id(
        &self,
        code_intel: &CodeIntelligenceService,
        file_id: i64,
        file_path: &Path,
    ) -> Result<()> {
        let content = tokio::fs::read_to_string(file_path).await
            .map_err(|e| anyhow::anyhow!("Failed to read file {}: {}", file_path.display(), e))?;

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

fn is_rust_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("rs"))
        .unwrap_or(false)
}

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
