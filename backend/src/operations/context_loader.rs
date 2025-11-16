// src/operations/context_loader.rs
// Shared service for loading project context (file tree, code intelligence)
// Eliminates duplication between operation engine and unified handler

use std::sync::Arc;
use tracing::{debug, warn};

use crate::git::FileNode;
use crate::git::client::GitClient;
use crate::git::client::project_ops::ProjectOps;
use crate::memory::core::types::MemoryEntry;
use crate::memory::features::code_intelligence::CodeIntelligenceService;

/// Service for loading various types of project context
pub struct ContextLoader {
    git_client: GitClient,
    code_intelligence: Arc<CodeIntelligenceService>,
}

impl ContextLoader {
    pub fn new(git_client: GitClient, code_intelligence: Arc<CodeIntelligenceService>) -> Self {
        Self {
            git_client,
            code_intelligence,
        }
    }

    /// Load file tree for a project
    /// Returns None if project_id is None or if loading fails
    pub async fn load_file_tree(&self, project_id: Option<&str>) -> Option<Vec<FileNode>> {
        let pid = project_id?;
        debug!("Loading file tree for project {}", pid);

        match self.git_client.get_project_tree(pid).await {
            Ok(tree) => {
                debug!("Loaded file tree with {} items", tree.len());
                Some(tree)
            }
            Err(e) => {
                warn!("Failed to load file tree: {}", e);
                None
            }
        }
    }

    /// Load code intelligence context based on user query
    /// Returns None if project_id is None or if no relevant code is found
    pub async fn load_code_context(
        &self,
        user_content: &str,
        project_id: Option<&str>,
        limit: usize,
    ) -> Option<Vec<MemoryEntry>> {
        let pid = project_id?;
        debug!("Loading code intelligence context for project {}", pid);

        match self
            .code_intelligence
            .search_code(user_content, pid, limit)
            .await
        {
            Ok(entries) if !entries.is_empty() => {
                debug!("Loaded {} code intelligence entries", entries.len());
                Some(entries)
            }
            Ok(_) => {
                debug!("No relevant code elements found");
                None
            }
            Err(e) => {
                warn!("Failed to load code context: {}", e);
                None
            }
        }
    }

    /// Load both file tree and code context in one call
    /// Returns tuple of (file_tree, code_context)
    pub async fn load_project_context(
        &self,
        user_content: &str,
        project_id: Option<&str>,
        code_limit: usize,
    ) -> (Option<Vec<FileNode>>, Option<Vec<MemoryEntry>>) {
        let project_id_str = match project_id {
            Some(pid) => pid,
            None => return (None, None),
        };

        // Load both concurrently
        let (file_tree_result, code_context_result) = tokio::join!(
            self.load_file_tree(Some(project_id_str)),
            self.load_code_context(user_content, Some(project_id_str), code_limit)
        );

        (file_tree_result, code_context_result)
    }
}
