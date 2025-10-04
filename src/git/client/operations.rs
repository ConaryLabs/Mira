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
                // Check if file is analyzable (Rust, TypeScript, or JavaScript)
                if !is_rust_file(file_path) && !is_typescript_file(file_path) && !is_javascript_file(file_path) {
                    continue;
                }

                // Use project-aware analysis to enable WebSocket detection
                match self.analyze_file_with_project(
                    code_intel, 
                    *file_id, 
                    file_path,
                    &attachment.project_id  // Pass project_id for WebSocket analysis
                ).await {
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
        let line_count = content_str.lines().count() as i64;
        
        // Detect language based on file extension
        let language = if is_rust_file(file_path) {
            Some("rust".to_string())
        } else if is_typescript_file(file_path) {
            Some("typescript".to_string())
        } else if is_javascript_file(file_path) {
            Some("javascript".to_string())
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

    async fn analyze_file_with_project(
        &self,
        code_intel: &CodeIntelligenceService,
        file_id: i64,
        file_path: &Path,
        project_id: &str,  // NEW: project_id for WebSocket analysis
    ) -> Result<()> {
        let content = tokio::fs::read_to_string(file_path).await
            .map_err(|e| anyhow::anyhow!("Failed to read file {}: {}", file_path.display(), e))?;

        let file_path_str = file_path.to_string_lossy();
        
        // Determine language from file extension
        let language = if is_rust_file(file_path) {
            "rust"
        } else if is_typescript_file(file_path) {
            "typescript"
        } else if is_javascript_file(file_path) {
            "javascript"
        } else {
            return Ok(()); // Skip unsupported file types
        };
        
        // Use project-aware analysis to enable WebSocket detection
        let result = code_intel.analyze_and_store_with_project(
            file_id,
            &content,
            &file_path_str,
            language,
            project_id,  // Enables WebSocket call/handler detection
        ).await?;

        debug!(
            "Analyzed {} file {} (id: {}): {} elements, complexity: {}, {} quality issues",
            language,
            file_path.display(),
            file_id,
            result.elements_count,
            result.complexity_score,
            result.quality_issues_count
        );

        Ok(())
    }

    pub async fn sync_changes(&self, attachment: &GitRepoAttachment, commit_message: &str) -> Result<()> {
        info!("Syncing changes for repository {}", attachment.id);
        
        // First pull
        self.pull_changes(attachment).await?;
        
        // Then commit and push
        self.commit_and_push(attachment, commit_message).await?;
        
        info!("Successfully synced changes for repository {}", attachment.id);
        Ok(())
    }

    pub async fn pull_changes(&self, attachment: &GitRepoAttachment) -> Result<()> {
        info!("Pulling latest changes for repository {}", attachment.id);

        let local_path = attachment.local_path.clone();
        
        tokio::task::spawn_blocking(move || {
            let repo = Repository::open(&local_path)?;
            let mut remote = repo.find_remote("origin")?;
            remote.fetch(&["main"], None, None)?;
            
            let fetch_head = repo.find_reference("FETCH_HEAD")?;
            let fetch_commit = repo.reference_to_annotated_commit(&fetch_head)?;
            
            let analysis = repo.merge_analysis(&[&fetch_commit])?;
            
            if analysis.0.is_fast_forward() {
                let refname = "refs/heads/main";
                let mut reference = repo.find_reference(refname)?;
                reference.set_target(fetch_commit.id(), "Fast-Forward")?;
                repo.set_head(refname)?;
                repo.checkout_head(Some(git2::build::CheckoutBuilder::default().force()))?;
            }
            
            Ok::<(), anyhow::Error>(())
        })
        .await
        .into_api_error("Failed to spawn blocking task")?
        .into_api_error("Failed to pull changes")?;

        self.store.update_last_sync(&attachment.id, Utc::now())
            .await
            .into_api_error("Failed to update last sync time")?;

        info!("Successfully pulled changes for repository {}", attachment.id);
        Ok(())
    }
    
    pub async fn pull_changes_by_id(&self, attachment_id: &str) -> ApiResult<()> {
        let attachment = self.store.get_attachment(attachment_id).await
            .into_api_error("Failed to get attachment")?
            .ok_or_else(|| anyhow::anyhow!("Attachment not found"))
            .into_api_error("Attachment not found")?;
            
        self.pull_changes(&attachment).await
            .into_api_error("Failed to pull changes")
    }

    pub async fn commit_and_push(&self, attachment: &GitRepoAttachment, message: &str) -> Result<()> {
        info!("Committing and pushing changes for repository {}", attachment.id);

        let local_path = attachment.local_path.clone();
        let commit_message = message.to_string();
        
        tokio::task::spawn_blocking(move || {
            let repo = Repository::open(&local_path)?;
            let mut index = repo.index()?;
            index.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)?;
            index.write()?;
            
            let oid = index.write_tree()?;
            let signature = repo.signature()?;
            let tree = repo.find_tree(oid)?;
            let parent_commit = repo.head()?.peel_to_commit()?;
            
            repo.commit(
                Some("HEAD"),
                &signature,
                &signature,
                &commit_message,
                &tree,
                &[&parent_commit],
            )?;
            
            let mut remote = repo.find_remote("origin")?;
            remote.push(&["refs/heads/main:refs/heads/main"], None)?;
            
            Ok::<(), anyhow::Error>(())
        })
        .await
        .into_api_error("Failed to spawn blocking task")?
        .into_api_error("Failed to commit and push")?;

        info!("Successfully committed and pushed changes for repository {}", attachment.id);
        Ok(())
    }

    pub async fn restore_file(&self, attachment_id: &str, file_path: &str) -> Result<()> {
        info!("Restoring file {} in repository {}", file_path, attachment_id);

        let attachment = self.store.get_attachment(attachment_id).await
            .into_api_error("Failed to get attachment")?
            .ok_or_else(|| anyhow::anyhow!("Attachment not found"))?;

        let local_path = attachment.local_path.clone();
        let file_path_owned = file_path.to_string();

        tokio::task::spawn_blocking(move || -> Result<()> {
            let repo = Repository::open(&local_path)?;
            let head = repo.head()?;
            let tree = head.peel_to_tree()?;
            
            let entry = tree.get_path(Path::new(&file_path_owned))?;
            let blob = repo.find_blob(entry.id())?;
            
            let full_file_path = Path::new(&local_path).join(&file_path_owned);
            if let Some(parent) = full_file_path.parent() {
                fs::create_dir_all(parent)?;
            }
            
            fs::write(&full_file_path, blob.content())?;
            
            Ok(())
        })
        .await
        .into_api_error("Failed to spawn blocking task")?
        .into_api_error("Failed to restore file")?;

        info!("Successfully restored file {} in repository {}", file_path, attachment_id);
        Ok(())
    }

    pub async fn hard_reset(&self, attachment_id: &str) -> Result<()> {
        let attachment = self.store.get_attachment(attachment_id).await
            .into_api_error("Failed to get attachment")?
            .ok_or_else(|| anyhow::anyhow!("Attachment not found"))?;
            
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

    pub async fn reset_to_remote(&self, attachment_id: &str) -> ApiResult<()> {
        self.hard_reset(attachment_id).await
            .into_api_error("Failed to reset to remote")
    }
}

fn is_rust_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("rs"))
        .unwrap_or(false)
}

fn is_typescript_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e == "ts" || e == "tsx")
        .unwrap_or(false)
}

fn is_javascript_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e == "js" || e == "jsx" || e == "mjs")
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
    
    // Common directories to ignore
    let ignored_dirs = [
        "/.git/", "/node_modules/", "/target/", "/dist/", "/build/", 
        "/.next/", "/.nuxt/", "/out/", "/coverage/", "/.cache/",
        "/vendor/", "/venv/", "/.venv/", "/env/", "/.pytest_cache/",
        "/__pycache__/", "/.idea/", "/.vscode/", "/.DS_Store",
        "/tmp/", "/temp/", "/logs/", "/.turbo/", "/pkg/",
    ];
    
    for dir in &ignored_dirs {
        if path_str.contains(dir) || path_str.starts_with(&dir[1..]) {
            return true;
        }
    }

    // Binary and media files
    if let Some(extension) = path.extension() {
        let ext = extension.to_string_lossy().to_lowercase();
        matches!(ext.as_str(),
            "exe" | "dll" | "so" | "dylib" | "bin" | "jar" | "zip" | "tar" | "gz" | 
            "png" | "jpg" | "jpeg" | "gif" | "bmp" | "ico" | "pdf" | "mp3" | "mp4" | "avi" |
            "woff" | "woff2" | "ttf" | "eot" | "svg" | "webp" | // fonts/images
            "lock" | "sum" // lock files
        )
    } else {
        false
    }
}
