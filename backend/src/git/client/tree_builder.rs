// src/git/client/tree_builder.rs
// File tree operations: browse, read, and write files in git repository
// Extracted from monolithic GitClient for focused responsibility

use anyhow::Result;
use git2::Repository;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

use crate::git::error::IntoGitErrorResult;
use crate::git::types::GitRepoAttachment;

/// File node in the repository tree
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileNode {
    pub name: String,
    pub path: String,
    pub node_type: FileNodeType,
    pub children: Vec<FileNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum FileNodeType {
    File,
    Directory,
}

/// Handles file tree operations
#[derive(Clone)]
pub struct TreeBuilder;

impl Default for TreeBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl TreeBuilder {
    /// Create new tree builder
    pub fn new() -> Self {
        Self
    }

    /// Get the file tree of a repository
    pub fn get_file_tree(&self, attachment: &GitRepoAttachment) -> Result<Vec<FileNode>> {
        let repo =
            Repository::open(&attachment.local_path).into_git_error("Failed to open repository")?;

        let head = repo
            .head()
            .into_git_error("Failed to get repository head")?;

        let tree = head
            .peel_to_tree()
            .into_git_error("Failed to get tree from head")?;

        let mut nodes = Vec::new();

        tree.walk(git2::TreeWalkMode::PreOrder, |root, entry| {
            let name = entry.name().unwrap_or("").to_string();
            let path = if root.is_empty() {
                name.clone()
            } else {
                format!("{root}/{name}")
            };

            let node_type = if entry.kind() == Some(git2::ObjectType::Tree) {
                FileNodeType::Directory
            } else {
                FileNodeType::File
            };

            nodes.push(FileNode {
                name,
                path,
                node_type,
                children: Vec::new(),
            });

            git2::TreeWalkResult::Ok
        })
        .into_git_error("Failed to walk repository tree")?;

        Ok(self.build_tree_structure(nodes))
    }

    /// Get file content from the repository
    pub fn get_file_content(
        &self,
        attachment: &GitRepoAttachment,
        file_path: &str,
    ) -> Result<String> {
        let full_path = Path::new(&attachment.local_path).join(file_path);

        fs::read_to_string(full_path)
            .into_git_error("Failed to read file content")
            .map_err(|api_err| anyhow::Error::msg(api_err.to_string()))
    }

    /// Update file content in the repository
    pub fn update_file_content(
        &self,
        attachment: &GitRepoAttachment,
        file_path: &str,
        content: &str,
        _commit_message: Option<&str>,
    ) -> Result<()> {
        let full_path = Path::new(&attachment.local_path).join(file_path);

        // Ensure parent directory exists
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).into_git_error("Failed to create parent directory")?;
        }

        fs::write(&full_path, content).into_git_error("Failed to write file content")?;

        // For MVP, we don't auto-commit
        // In the future, we could use commit_message to create a commit
        Ok(())
    }

    /// Build hierarchical tree structure from flat list of nodes
    fn build_tree_structure(&self, nodes: Vec<FileNode>) -> Vec<FileNode> {
        let mut root_nodes = Vec::new();

        // For now, return a flat structure
        // In the future, we could build a proper hierarchy
        root_nodes.extend(nodes);

        root_nodes
    }
}
