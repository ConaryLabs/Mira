// src/git/client/diff_parser.rs
// Diff parsing and commit comparison operations
// Extracted from monolithic GitClient for focused responsibility

use anyhow::Result;
use git2::{Oid, Repository};
use serde::{Deserialize, Serialize};

use crate::api::error::IntoApiError;
use crate::git::types::GitRepoAttachment;

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

/// Handles diff parsing and commit comparison
#[derive(Clone)]
pub struct DiffParser;

impl Default for DiffParser {
    fn default() -> Self {
        Self::new()
    }
}

impl DiffParser {
    /// Create new diff parser
    pub fn new() -> Self {
        Self
    }

    /// Get diff information for a specific commit
    pub fn get_commit_diff(
        &self,
        attachment: &GitRepoAttachment,
        commit_id: &str,
    ) -> Result<DiffInfo> {
        let repo =
            Repository::open(&attachment.local_path).into_api_error("Failed to open repository")?;

        let oid = Oid::from_str(commit_id).into_api_error("Invalid commit ID")?;

        let commit = repo.find_commit(oid).into_api_error("Commit not found")?;

        // Get parent commit (if any)
        let parent_tree = if commit.parent_count() > 0 {
            Some(
                commit
                    .parent(0)
                    .into_api_error("Failed to get parent commit")?
                    .tree()
                    .into_api_error("Failed to get parent tree")?,
            )
        } else {
            None
        };

        let commit_tree = commit.tree().into_api_error("Failed to get commit tree")?;

        // Create diff
        let mut diff = repo
            .diff_tree_to_tree(parent_tree.as_ref(), Some(&commit_tree), None)
            .into_api_error("Failed to create diff")?;

        // Get statistics
        let stats = diff.stats().into_api_error("Failed to get diff stats")?;
        let total_additions = stats.insertions();
        let total_deletions = stats.deletions();

        // Collect file changes
        let mut files = Vec::new();
        let num_deltas = diff.deltas().count();

        for idx in 0..num_deltas {
            if let Some(delta) = diff.get_delta(idx) {
                let path = delta
                    .new_file()
                    .path()
                    .and_then(|p| p.to_str())
                    .unwrap_or("")
                    .to_string();

                let old_path = if delta.old_file().path() != delta.new_file().path() {
                    delta
                        .old_file()
                        .path()
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
                    Ok(Some(patch)) => match patch.line_stats() {
                        Ok((_, adds, dels)) => (adds, dels),
                        Err(_) => (0, 0),
                    },
                    _ => (0, 0),
                };

                files.push(FileDiff {
                    path,
                    old_path,
                    status,
                    additions,
                    deletions,
                    hunks: Vec::new(), // Can be enhanced later for detailed hunks
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
        let repo =
            Repository::open(&attachment.local_path).into_api_error("Failed to open repository")?;

        let oid = Oid::from_str(commit_id).into_api_error("Invalid commit ID")?;

        let commit = repo.find_commit(oid).into_api_error("Commit not found")?;

        let tree = commit.tree().into_api_error("Failed to get commit tree")?;

        // Navigate to the file in the tree
        let entry = tree
            .get_path(std::path::Path::new(file_path))
            .into_api_error("File not found in commit")?;

        let blob = repo
            .find_blob(entry.id())
            .into_api_error("Failed to find blob")?;

        let content =
            std::str::from_utf8(blob.content()).into_api_error("File contains invalid UTF-8")?;

        Ok(content.to_string())
    }

    /// Get diff between two commits (for future use)
    pub fn get_diff_between_commits(
        &self,
        attachment: &GitRepoAttachment,
        from_commit: &str,
        to_commit: &str,
    ) -> Result<DiffInfo> {
        let repo =
            Repository::open(&attachment.local_path).into_api_error("Failed to open repository")?;

        let from_oid = Oid::from_str(from_commit).into_api_error("Invalid from commit ID")?;
        let to_oid = Oid::from_str(to_commit).into_api_error("Invalid to commit ID")?;

        let from_commit_obj = repo
            .find_commit(from_oid)
            .into_api_error("From commit not found")?;
        let to_commit_obj = repo
            .find_commit(to_oid)
            .into_api_error("To commit not found")?;

        let from_tree = from_commit_obj
            .tree()
            .into_api_error("Failed to get from tree")?;
        let to_tree = to_commit_obj
            .tree()
            .into_api_error("Failed to get to tree")?;

        // Create diff between the two trees
        let mut diff = repo
            .diff_tree_to_tree(Some(&from_tree), Some(&to_tree), None)
            .into_api_error("Failed to create diff")?;

        // Get statistics
        let stats = diff.stats().into_api_error("Failed to get diff stats")?;
        let total_additions = stats.insertions();
        let total_deletions = stats.deletions();

        // Collect file changes (similar to get_commit_diff)
        let mut files = Vec::new();
        let num_deltas = diff.deltas().count();

        for idx in 0..num_deltas {
            if let Some(delta) = diff.get_delta(idx) {
                let path = delta
                    .new_file()
                    .path()
                    .and_then(|p| p.to_str())
                    .unwrap_or("")
                    .to_string();

                let old_path = if delta.old_file().path() != delta.new_file().path() {
                    delta
                        .old_file()
                        .path()
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

                let (additions, deletions) = match git2::Patch::from_diff(&mut diff, idx) {
                    Ok(Some(patch)) => match patch.line_stats() {
                        Ok((_, adds, dels)) => (adds, dels),
                        Err(_) => (0, 0),
                    },
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
            commit_id: format!("{from_commit}..{to_commit}"),
            files_changed: files,
            additions: total_additions,
            deletions: total_deletions,
        })
    }
}
