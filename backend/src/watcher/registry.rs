// src/watcher/registry.rs
// Registry for watched repositories

use anyhow::Result;
use notify::RecursiveMode;
use notify_debouncer_full::{Debouncer, RecommendedCache};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, info, warn};

/// Information about a watched repository
#[derive(Debug, Clone)]
pub struct WatchedRepository {
    pub attachment_id: String,
    pub project_id: String,
    pub path: PathBuf,
    /// Last time a git operation completed (for cooldown)
    pub last_git_operation: Option<Instant>,
}

/// Registry of watched repositories and their paths
pub struct WatchRegistry {
    /// Map of attachment_id -> WatchedRepository
    repositories: RwLock<HashMap<String, WatchedRepository>>,
    /// Map of path -> attachment_id for reverse lookup
    path_to_attachment: RwLock<HashMap<PathBuf, String>>,
    /// Paths that are pending to be watched (before debouncer is set)
    pending_watches: RwLock<Vec<PathBuf>>,
    /// The debouncer instance (set after creation)
    debouncer: RwLock<Option<Arc<parking_lot::Mutex<Debouncer<notify::RecommendedWatcher, RecommendedCache>>>>>,
}

impl WatchRegistry {
    pub fn new() -> Self {
        Self {
            repositories: RwLock::new(HashMap::new()),
            path_to_attachment: RwLock::new(HashMap::new()),
            pending_watches: RwLock::new(Vec::new()),
            debouncer: RwLock::new(None),
        }
    }

    /// Set the debouncer instance (called after debouncer creation)
    pub fn set_debouncer(&self, debouncer: Arc<parking_lot::Mutex<Debouncer<notify::RecommendedWatcher, RecommendedCache>>>) {
        // Process any pending watches
        let pending = {
            let mut pending = self.pending_watches.write();
            std::mem::take(&mut *pending)
        };

        // Add pending watches
        for path in pending {
            let mut d = debouncer.lock();
            if let Err(e) = d.watch(&path, RecursiveMode::Recursive) {
                warn!("Failed to add pending watch for {:?}: {}", path, e);
            } else {
                debug!("Added pending watch for {:?}", path);
            }
        }

        let mut d = self.debouncer.write();
        *d = Some(debouncer);
    }

    /// Register a repository to watch
    pub async fn watch_repository(
        &self,
        attachment_id: String,
        project_id: String,
        path: PathBuf,
    ) -> Result<()> {
        info!(
            "Registering watch for repository: {} at {:?}",
            attachment_id, path
        );

        // Verify path exists
        if !path.exists() {
            return Err(anyhow::anyhow!("Path does not exist: {:?}", path));
        }

        // Add to registry
        {
            let mut repos = self.repositories.write();
            let mut paths = self.path_to_attachment.write();

            repos.insert(
                attachment_id.clone(),
                WatchedRepository {
                    attachment_id: attachment_id.clone(),
                    project_id,
                    path: path.clone(),
                    last_git_operation: None,
                },
            );
            paths.insert(path.clone(), attachment_id.clone());
        }

        // Add watch to the debouncer
        let debouncer_guard = self.debouncer.read();
        if let Some(ref debouncer) = *debouncer_guard {
            let mut d = debouncer.lock();
            if let Err(e) = d.watch(&path, RecursiveMode::Recursive) {
                warn!("Failed to add watch for {:?}: {}", path, e);
                // Remove from registry on failure
                let mut repos = self.repositories.write();
                let mut paths = self.path_to_attachment.write();
                repos.remove(&attachment_id);
                paths.remove(&path);
                return Err(e.into());
            }
            debug!("Added recursive watch for {:?}", path);
        } else {
            // Debouncer not ready yet, add to pending
            let mut pending = self.pending_watches.write();
            pending.push(path);
            debug!("Added to pending watches (debouncer not ready)");
        }

        Ok(())
    }

    /// Unregister a repository from watching
    pub async fn unwatch_repository(&self, attachment_id: &str) -> Result<()> {
        info!("Unregistering watch for repository: {}", attachment_id);

        let path = {
            let repos = self.repositories.read();
            repos.get(attachment_id).map(|r| r.path.clone())
        };

        if let Some(path) = path {
            // Remove watch from debouncer
            let debouncer_guard = self.debouncer.read();
            if let Some(ref debouncer) = *debouncer_guard {
                let mut d = debouncer.lock();
                if let Err(e) = d.unwatch(&path) {
                    warn!("Failed to remove watch for {:?}: {}", path, e);
                }
            }

            // Remove from registry
            let mut repos = self.repositories.write();
            let mut paths = self.path_to_attachment.write();
            repos.remove(attachment_id);
            paths.remove(&path);
        }

        Ok(())
    }

    /// Find which repository a path belongs to
    ///
    /// Returns (attachment_id, project_id, base_path) if found
    pub fn find_repository_for_path(&self, path: &PathBuf) -> Option<(String, String, PathBuf)> {
        let repos = self.repositories.read();

        for repo in repos.values() {
            if path.starts_with(&repo.path) {
                return Some((
                    repo.attachment_id.clone(),
                    repo.project_id.clone(),
                    repo.path.clone(),
                ));
            }
        }

        None
    }

    /// Mark that a git operation just completed for a repository
    pub fn mark_git_operation(&self, attachment_id: &str) {
        let mut repos = self.repositories.write();
        if let Some(repo) = repos.get_mut(attachment_id) {
            repo.last_git_operation = Some(Instant::now());
            debug!("Marked git operation for {}", attachment_id);
        }
    }

    /// Check if we're in the git operation cooldown period
    pub fn in_git_cooldown(&self, attachment_id: &str, cooldown_ms: u64) -> bool {
        let repos = self.repositories.read();
        if let Some(repo) = repos.get(attachment_id) {
            if let Some(last_op) = repo.last_git_operation {
                let elapsed = last_op.elapsed().as_millis() as u64;
                return elapsed < cooldown_ms;
            }
        }
        false
    }

    /// Get all watched repositories
    pub fn get_all_repositories(&self) -> Vec<WatchedRepository> {
        let repos = self.repositories.read();
        repos.values().cloned().collect()
    }

    /// Get count of watched repositories
    pub fn count(&self) -> usize {
        let repos = self.repositories.read();
        repos.len()
    }
}

impl Default for WatchRegistry {
    fn default() -> Self {
        Self::new()
    }
}
