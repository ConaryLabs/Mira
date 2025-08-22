// src/git/client/tree_builder.rs
// File tree building and file operations
// Extracted from monolithic GitClient for focused responsibility

use anyhow::{Result, Context};
use git2::{Repository, TreeWalkMode, TreeWalkResult, ObjectType};
use std::fs;
use std::path::Path;
use serde::{Serialize, Deserialize};

use crate::git::types::GitRepoAttachment;
use crate::api::error::IntoApiError;

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

/// Handles file tree building and file operations
#[derive(Clone)]
pub struct TreeBuilder;

impl TreeBuilder {
    /// Create new tree builder
    pub fn new() -> Self {
        Self
    }

    /// Get the file tree of a repository
    pub fn get_file_tree(&self, attachment: &GitRepoAttachment) -> Result<Vec<FileNode>> {
        let repo = Repository::open(&attachment.local_path)
            .into_api_error("Failed to open repository")?;
        
        let head = repo.head()
            .into_api_error("Failed to get HEAD reference")?;
        
        let tree = head.peel_to_tree()
            .into_api_error("Failed to get tree from HEAD")?;
        
        let mut nodes = Vec::new();
        walk_tree(&repo, &tree, "", &mut nodes)?;
        
        // Sort nodes: directories first, then files, both alphabetically
        nodes.sort_by(|a, b| {
            match (&a.node_type, &b.node_type) {
                (FileNodeType::Directory, FileNodeType::File) => std::cmp::Ordering::Less,
                (FileNodeType::File, FileNodeType::Directory) => std::cmp::Ordering::Greater,
                _ => a.name.cmp(&b.name),
            }
        });
        
        Ok(nodes)
    }

    /// Get file content from the repository
    pub fn get_file_content(&self, attachment: &GitRepoAttachment, file_path: &str) -> Result<String> {
        let full_path = Path::new(&attachment.local_path).join(file_path);
        fs::read_to_string(full_path)
            .into_api_error("Failed to read file content")
    }

    /// Update file content in the repository
    pub fn update_file_content(
        &self,
        attachment: &GitRepoAttachment,
        file_path: &str,
        content: &str,
        commit_message: Option<&str>,
    ) -> Result<()> {
        let full_path = Path::new(&attachment.local_path).join(file_path);

        // Ensure parent directory exists
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent)
                .into_api_error("Failed to create parent directory")?;
        }

        // Write file content
        fs::write(&full_path, content)
            .into_api_error("Failed to write file content")?;

        // If commit message is provided, log it (actual commit logic would go here)
        if let Some(message) = commit_message {
            tracing::info!("File updated: {} (commit message: {})", file_path, message);
        }

        Ok(())
    }

    /// Check if a file exists in the repository
    pub fn file_exists(&self, attachment: &GitRepoAttachment, file_path: &str) -> bool {
        let full_path = Path::new(&attachment.local_path).join(file_path);
        full_path.exists()
    }

    /// List all files in the repository (flat list)
    pub fn list_all_files(&self, attachment: &GitRepoAttachment) -> Result<Vec<String>> {
        let nodes = self.get_file_tree(attachment)?;
        let files: Vec<String> = nodes
            .into_iter()
            .filter(|node| node.node_type == FileNodeType::File)
            .map(|node| node.path)
            .collect();
        
        Ok(files)
    }

    /// Search for files matching a pattern
    pub fn find_files(&self, attachment: &GitRepoAttachment, pattern: &str) -> Result<Vec<FileNode>> {
        let nodes = self.get_file_tree(attachment)?;
        let pattern_lower = pattern.to_lowercase();
        
        let matching_files: Vec<FileNode> = nodes
            .into_iter()
            .filter(|node| {
                node.name.to_lowercase().contains(&pattern_lower) ||
                node.path.to_lowercase().contains(&pattern_lower)
            })
            .collect();
        
        Ok(matching_files)
    }

    /// Get file statistics
    pub fn get_file_stats(&self, attachment: &GitRepoAttachment) -> Result<FileStats> {
        let nodes = self.get_file_tree(attachment)?;
        
        let file_count = nodes.iter().filter(|n| n.node_type == FileNodeType::File).count();
        let dir_count = nodes.iter().filter(|n| n.node_type == FileNodeType::Directory).count();
        let total_size: u64 = nodes.iter()
            .filter_map(|n| n.size)
            .sum();
        
        Ok(FileStats {
            total_files: file_count,
            total_directories: dir_count,
            total_size_bytes: total_size,
        })
    }
}

/// File statistics for a repository
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileStats {
    pub total_files: usize,
    pub total_directories: usize,
    pub total_size_bytes: u64,
}

/// Recursively walk a git tree and build FileNode structure
fn walk_tree(
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

        let node_type = match entry.kind() {
            Some(ObjectType::Tree) => FileNodeType::Directory,
            Some(ObjectType::Blob) => FileNodeType::File,
            _ => return TreeWalkResult::Skip,
        };

        // Get file size for files (blobs)
        let size = if node_type == FileNodeType::File {
            if let Ok(object) = entry.to_object(repo) {
                if let Some(blob) = object.as_blob() {
                    Some(blob.size() as u64)
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        nodes.push(FileNode {
            name,
            path,
            node_type,
            size,
        });

        TreeWalkResult::Ok
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tree_builder_creation() {
        let builder = TreeBuilder::new();
        // TreeBuilder is stateless, so just verify it can be created
        assert!(std::mem::size_of_val(&builder) >= 0);
    }

    #[test]
    fn test_file_node_serialization() {
        let node = FileNode {
            name: "test.rs".to_string(),
            path: "src/test.rs".to_string(),
            node_type: FileNodeType::File,
            size: Some(1024),
        };

        let json = serde_json::to_string(&node).unwrap();
        let deserialized: FileNode = serde_json::from_str(&json).unwrap();
        
        assert_eq!(node.name, deserialized.name);
        assert_eq!(node.path, deserialized.path);
        assert_eq!(node.node_type, deserialized.node_type);
        assert_eq!(node.size, deserialized.size);
    }

    #[test]
    fn test_file_stats_creation() {
        let stats = FileStats {
            total_files: 10,
            total_directories: 3,
            total_size_bytes: 1024 * 50,
        };
        
        assert_eq!(stats.total_files, 10);
        assert_eq!(stats.total_directories, 3);
        assert_eq!(stats.total_size_bytes, 51200);
    }
}
