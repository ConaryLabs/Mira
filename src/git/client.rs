use anyhow::{Result, Context};
use git2::{Repository, BranchType, Oid, ObjectType, TreeWalkMode, TreeWalkResult};
use std::fs;
use std::path::{Path, PathBuf};
use chrono::{Utc, DateTime};
use uuid::Uuid;
use serde::{Serialize, Deserialize};

use super::types::{GitRepoAttachment, GitImportStatus};
use super::store::GitStore;

/// File node for tree representation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileNode {
    pub name: String,
    pub path: String,
    pub node_type: FileNodeType,
    pub size: Option<u64>,
}

#[derive(PartialEq, Eq, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileNodeType {
    File,
    Directory,
}

/// Branch information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchInfo {
    pub name: String,
    pub is_current: bool,
    pub last_commit_id: String,
    pub last_commit_message: String,
    pub last_commit_time: i64,
}

/// Commit information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitInfo {
    pub id: String,
    pub message: String,
    pub author: String,
    pub email: String,
    pub timestamp: i64,
    pub parent_ids: Vec<String>,
}

/// Diff information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffInfo {
    pub commit_id: String,
    pub files_changed: Vec<FileDiff>,
    pub additions: usize,
    pub deletions: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileDiff {
    pub path: String,
    pub old_path: Option<String>,
    pub status: DiffStatus,
    pub additions: usize,
    pub deletions: usize,
    pub hunks: Vec<DiffHunk>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DiffStatus {
    Added,
    Deleted,
    Modified,
    Renamed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffHunk {
    pub header: String,
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffLine {
    pub content: String,
    pub line_type: DiffLineType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DiffLineType {
    Addition,
    Deletion,
    Context,
}

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

    // ===== NEW METHODS FOR PHASE 3 =====

    /// Get the file tree of a repository
    pub fn get_file_tree(&self, attachment: &GitRepoAttachment) -> Result<Vec<FileNode>> {
        let repo = Repository::open(&attachment.local_path)
            .context("Failed to open repository")?;
        
        let head = repo.head()
            .context("Failed to get HEAD reference")?;
        
        let tree = head.peel_to_tree()
            .context("Failed to get tree from HEAD")?;
        
        let mut nodes = Vec::new();
        self.walk_tree(&repo, &tree, "", &mut nodes)?;
        
        Ok(nodes)
    }

    /// Recursively walk a git tree and build FileNode structure
    fn walk_tree(
        &self,
        repo: &Repository,
        tree: &git2::Tree,
        _base_path: &str,
        nodes: &mut Vec<FileNode>,
    ) -> Result<()> {
        tree.walk(TreeWalkMode::PreOrder, |dir_path, entry| {
            let name = entry.name().unwrap_or("").to_string();
            let path = if dir_path.is_empty() {
                name.clone()
            } else {
                format!("{}/{}", dir_path, name)
            };

            match entry.kind() {
                Some(ObjectType::Tree) => {
                    nodes.push(FileNode {
                        name,
                        path,
                        node_type: FileNodeType::Directory,
                        size: None,
                    });
                }
                Some(ObjectType::Blob) => {
                    let size = entry.to_object(repo)
                        .ok()
                        .and_then(|obj| obj.as_blob().map(|blob| blob.size() as u64));
                    
                    nodes.push(FileNode {
                        name,
                        path,
                        node_type: FileNodeType::File,
                        size,
                    });
                }
                _ => {}
            }
            
            TreeWalkResult::Ok
        })?;
        
        Ok(())
    }

    /// Get list of branches
    pub fn get_branches(&self, attachment: &GitRepoAttachment) -> Result<Vec<BranchInfo>> {
        let repo = Repository::open(&attachment.local_path)
            .context("Failed to open repository")?;
        
        let current_branch = repo.head()
            .ok()
            .and_then(|head| head.shorthand().map(String::from));
        
        let mut branches = Vec::new();
        
        // Get local branches
        for branch_result in repo.branches(Some(BranchType::Local))? {
            let (branch, _) = branch_result?;
            
            if let Some(name) = branch.name()? {
                let is_current = current_branch.as_ref() == Some(&name.to_string());
                
                // Get last commit info
                let commit = branch.get().peel_to_commit()?;
                
                branches.push(BranchInfo {
                    name: name.to_string(),
                    is_current,
                    last_commit_id: commit.id().to_string(),
                    last_commit_message: commit.message().unwrap_or("").to_string(),
                    last_commit_time: commit.time().seconds(),
                });
            }
        }
        
        Ok(branches)
    }

    /// Switch to a different branch
    pub fn switch_branch(&self, attachment: &GitRepoAttachment, branch_name: &str) -> Result<()> {
        let repo = Repository::open(&attachment.local_path)
            .context("Failed to open repository")?;
        
        // Find the branch
        let branch = repo.find_branch(branch_name, BranchType::Local)
            .context(format!("Branch '{}' not found", branch_name))?;
        
        // Set HEAD to the branch
        repo.set_head(branch.get().name().unwrap())
            .context("Failed to set HEAD")?;
        
        // Checkout the branch
        repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
            .context("Failed to checkout branch")?;
        
        Ok(())
    }

    /// Get commit history
    pub fn get_commits(&self, attachment: &GitRepoAttachment, limit: usize) -> Result<Vec<CommitInfo>> {
        let repo = Repository::open(&attachment.local_path)
            .context("Failed to open repository")?;
        
        let mut revwalk = repo.revwalk()?;
        revwalk.push_head()?;
        
        let commits: Result<Vec<_>> = revwalk
            .take(limit)
            .map(|oid_result| {
                let oid = oid_result?;
                let commit = repo.find_commit(oid)?;
                
                let parent_ids = commit.parent_ids()
                    .map(|id| id.to_string())
                    .collect();
                
                Ok(CommitInfo {
                    id: commit.id().to_string(),
                    message: commit.message().unwrap_or("").to_string(),
                    author: commit.author().name().unwrap_or("Unknown").to_string(),
                    email: commit.author().email().unwrap_or("").to_string(),
                    timestamp: commit.time().seconds(),
                    parent_ids,
                })
            })
            .collect();
        
        commits
    }

    /// Get diff for a specific commit
    pub fn get_diff(&self, attachment: &GitRepoAttachment, commit_id: &str) -> Result<DiffInfo> {
        let repo = Repository::open(&attachment.local_path)
            .context("Failed to open repository")?;
        
        let oid = Oid::from_str(commit_id)
            .context("Invalid commit ID")?;
        
        let commit = repo.find_commit(oid)
            .context("Commit not found")?;
        
        // Get parent commit (if any)
        let parent_tree = if commit.parent_count() > 0 {
            Some(commit.parent(0)?.tree()?)
        } else {
            None
        };
        
        let commit_tree = commit.tree()?;
        
        // Create diff
        let mut diff = repo.diff_tree_to_tree(
            parent_tree.as_ref(),
            Some(&commit_tree),
            None,
        )?;
        
        // Get statistics
        let stats = diff.stats()?;
        let total_additions = stats.insertions();
        let total_deletions = stats.deletions();
        
        // Collect file changes
        let mut files = Vec::new();
        let num_deltas = diff.deltas().count();
        
        for idx in 0..num_deltas {
            if let Some(delta) = diff.get_delta(idx) {
                let path = delta.new_file().path()
                    .and_then(|p| p.to_str())
                    .unwrap_or("")
                    .to_string();
                
                let old_path = if delta.old_file().path() != delta.new_file().path() {
                    delta.old_file().path()
                        .and_then(|p| p.to_str())
                        .map(String::from)
                } else {
                    None
                };
                
                let status = match delta.status() {
                    git2::Delta::Added => DiffStatus::Added,
                    git2::Delta::Deleted => DiffStatus::Deleted,
                    git2::Delta::Modified => DiffStatus::Modified,
                    git2::Delta::Renamed => DiffStatus::Renamed,
                    _ => DiffStatus::Modified,
                };
                
                // Get per-file stats using patch
                let (additions, deletions) = match git2::Patch::from_diff(&mut diff, idx) {
                    Ok(Some(patch)) => {
                        match patch.line_stats() {
                            Ok((_, adds, dels)) => (adds, dels),
                            Err(_) => (0, 0),
                        }
                    }
                    _ => (0, 0),
                };
                
                files.push(FileDiff {
                    path,
                    old_path,
                    status,
                    additions,
                    deletions,
                    hunks: Vec::new(),
                });
            }
        }
        
        Ok(DiffInfo {
            commit_id: commit_id.to_string(),
            files_changed: files,
            additions: total_additions,
            deletions: total_deletions,
        })
    }

    /// Get file content at a specific commit
    pub fn get_file_at_commit(
        &self,
        attachment: &GitRepoAttachment,
        commit_id: &str,
        file_path: &str,
    ) -> Result<String> {
        let repo = Repository::open(&attachment.local_path)
            .context("Failed to open repository")?;
        
        let oid = Oid::from_str(commit_id)
            .context("Invalid commit ID")?;
        
        let commit = repo.find_commit(oid)
            .context("Commit not found")?;
        
        let tree = commit.tree()?;
        
        let entry = tree.get_path(Path::new(file_path))
            .context("File not found in commit")?;
        
        let blob = repo.find_blob(entry.id())
            .context("Failed to find file blob")?;
        
        let content = std::str::from_utf8(blob.content())
            .context("File is not valid UTF-8")?
            .to_string();
        
        Ok(content)
    }
}
