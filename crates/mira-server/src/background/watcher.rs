// crates/mira-server/src/background/watcher.rs
// File system watcher for automatic incremental indexing

use super::code_health;
use crate::db::Database;
use crate::indexer;
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, watch, RwLock};

/// Directories to skip when watching
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "target",
    ".git",
    "pkg",
    "dist",
    "build",
    "vendor",
    "__pycache__",
    ".next",
    "out",
    ".venv",
    "venv",
];

/// Supported file extensions
const SUPPORTED_EXTENSIONS: &[&str] = &["rs", "py", "ts", "tsx", "js", "jsx", "go"];

/// Debounce duration for rapid file changes
const DEBOUNCE_MS: u64 = 500;

/// File watcher manages watching multiple project directories
pub struct FileWatcher {
    db: Arc<Database>,
    /// Map of project_id -> project_path for active watches
    watched_projects: Arc<RwLock<HashMap<i64, PathBuf>>>,
    /// Pending file changes (debounced)
    pending_changes: Arc<RwLock<HashMap<PathBuf, (ChangeType, Instant)>>>,
    shutdown: watch::Receiver<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChangeType {
    Modified,
    Created,
    Deleted,
}

impl FileWatcher {
    pub fn new(
        db: Arc<Database>,
        shutdown: watch::Receiver<bool>,
    ) -> Self {
        Self {
            db,
            watched_projects: Arc::new(RwLock::new(HashMap::new())),
            pending_changes: Arc::new(RwLock::new(HashMap::new())),
            shutdown,
        }
    }

    /// Start watching a project directory
    pub async fn watch_project(&self, project_id: i64, project_path: PathBuf) {
        let mut projects = self.watched_projects.write().await;
        projects.entry(project_id).or_insert_with(|| {
            tracing::info!("Starting file watch for project {} at {:?}", project_id, project_path);
            project_path
        });
    }

    /// Stop watching a project
    pub async fn unwatch_project(&self, project_id: i64) {
        let mut projects = self.watched_projects.write().await;
        if let Some(path) = projects.remove(&project_id) {
            tracing::info!("Stopped file watch for project {} at {:?}", project_id, path);
        }
    }

    /// Run the file watcher loop
    pub async fn run(mut self) {
        tracing::info!("File watcher started");

        // Create channel for file system events
        let (tx, mut rx) = mpsc::channel::<(PathBuf, ChangeType)>(1000);

        // Clone for the watcher callback
        let tx_clone = tx.clone();

        // Create the file system watcher
        let mut watcher: RecommendedWatcher = match Watcher::new(
            move |res: Result<Event, notify::Error>| {
                if let Ok(event) = res {
                    let change_type = match event.kind {
                        EventKind::Create(_) => Some(ChangeType::Created),
                        EventKind::Modify(_) => Some(ChangeType::Modified),
                        EventKind::Remove(_) => Some(ChangeType::Deleted),
                        _ => None,
                    };

                    if let Some(ct) = change_type {
                        for path in event.paths {
                            if Self::should_process_path(&path) {
                                let _ = tx_clone.blocking_send((path, ct));
                            }
                        }
                    }
                }
            },
            Config::default().with_poll_interval(Duration::from_secs(2)),
        ) {
            Ok(w) => w,
            Err(e) => {
                tracing::error!("Failed to create file watcher: {}", e);
                return;
            }
        };

        // Track which paths are being watched
        let mut watched_paths: HashSet<PathBuf> = HashSet::new();

        loop {
            // Check for shutdown
            if *self.shutdown.borrow() {
                tracing::info!("File watcher shutting down");
                break;
            }

            // Update watched directories based on registered projects
            {
                let projects = self.watched_projects.read().await;
                for (_, project_path) in projects.iter() {
                    if !watched_paths.contains(project_path) {
                        if let Err(e) = watcher.watch(project_path, RecursiveMode::Recursive) {
                            tracing::warn!("Failed to watch {:?}: {}", project_path, e);
                        } else {
                            tracing::debug!("Now watching {:?}", project_path);
                            watched_paths.insert(project_path.clone());
                        }
                    }
                }

                // Unwatch removed projects
                let current_paths: HashSet<_> = projects.values().cloned().collect();
                for path in watched_paths.clone() {
                    if !current_paths.contains(&path) {
                        let _ = watcher.unwatch(&path);
                        watched_paths.remove(&path);
                        tracing::debug!("Stopped watching {:?}", path);
                    }
                }
            }

            // Process file events with timeout
            tokio::select! {
                Some((path, change_type)) = rx.recv() => {
                    self.queue_change(path, change_type).await;
                }
                _ = tokio::time::sleep(Duration::from_millis(100)) => {
                    // Process debounced changes
                    self.process_pending_changes().await;
                }
                _ = self.shutdown.changed() => {
                    if *self.shutdown.borrow() {
                        break;
                    }
                }
            }
        }
    }

    /// Check if a path should be processed
    fn should_process_path(path: &Path) -> bool {
        // Check extension
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !SUPPORTED_EXTENSIONS.contains(&ext) {
            return false;
        }

        // Check for skip directories in path
        for component in path.components() {
            if let std::path::Component::Normal(name) = component {
                let name_str = name.to_string_lossy();
                if name_str.starts_with('.') || SKIP_DIRS.contains(&name_str.as_ref()) {
                    return false;
                }
            }
        }

        true
    }

    /// Queue a file change for processing (with debounce)
    async fn queue_change(&self, path: PathBuf, change_type: ChangeType) {
        let mut pending = self.pending_changes.write().await;
        pending.insert(path, (change_type, Instant::now()));
    }

    /// Process pending changes after debounce period
    async fn process_pending_changes(&self) {
        let now = Instant::now();
        let debounce = Duration::from_millis(DEBOUNCE_MS);

        // Collect changes that have passed debounce period
        let ready: Vec<(PathBuf, ChangeType)> = {
            let pending = self.pending_changes.read().await;
            pending
                .iter()
                .filter(|(_, (_, timestamp))| now.duration_since(*timestamp) >= debounce)
                .map(|(path, (ct, _))| (path.clone(), *ct))
                .collect()
        };

        if ready.is_empty() {
            return;
        }

        // Remove processed changes
        {
            let mut pending = self.pending_changes.write().await;
            for (path, _) in &ready {
                pending.remove(path);
            }
        }

        // Process each change
        for (path, change_type) in ready {
            if let Err(e) = self.process_file_change(&path, change_type).await {
                tracing::warn!("Error processing file change {:?}: {}", path, e);
            }
        }
    }

    /// Process a single file change
    async fn process_file_change(&self, path: &Path, change_type: ChangeType) -> Result<(), String> {
        // Find which project this file belongs to
        let (project_id, relative_path) = {
            let projects = self.watched_projects.read().await;
            let mut found = None;
            for (pid, project_path) in projects.iter() {
                if path.starts_with(project_path) {
                    if let Ok(rel) = path.strip_prefix(project_path) {
                        found = Some((*pid, rel.to_path_buf()));
                        break;
                    }
                }
            }
            found.ok_or_else(|| format!("No project found for path {:?}", path))?
        };

        let rel_path_str = relative_path.to_string_lossy().to_string();

        match change_type {
            ChangeType::Deleted => {
                tracing::info!("File deleted: {}", rel_path_str);
                self.delete_file_data(project_id, &rel_path_str).await?;
            }
            ChangeType::Created | ChangeType::Modified => {
                tracing::info!("File {}: {}",
                    if change_type == ChangeType::Created { "created" } else { "modified" },
                    rel_path_str
                );
                self.update_file(project_id, path, &rel_path_str).await?;
            }
        }

        // Mark project for health rescan (will run on next background cycle)
        let _ = code_health::mark_health_scan_needed(&self.db, project_id);

        Ok(())
    }

    /// Delete all data associated with a file
    async fn delete_file_data(&self, project_id: i64, file_path: &str) -> Result<(), String> {
        let conn = self.db.conn();

        // Delete symbols
        conn.execute(
            "DELETE FROM code_symbols WHERE project_id = ? AND file_path = ?",
            rusqlite::params![project_id, file_path],
        ).map_err(|e| e.to_string())?;

        // Delete embeddings
        conn.execute(
            "DELETE FROM vec_code WHERE project_id = ? AND file_path = ?",
            rusqlite::params![project_id, file_path],
        ).map_err(|e| e.to_string())?;

        // Delete imports
        conn.execute(
            "DELETE FROM imports WHERE project_id = ? AND file_path = ?",
            rusqlite::params![project_id, file_path],
        ).map_err(|e| e.to_string())?;

        tracing::debug!("Deleted data for file {} in project {}", file_path, project_id);
        Ok(())
    }

    /// Update a file (re-parse and queue embeddings)
    async fn update_file(&self, project_id: i64, full_path: &Path, relative_path: &str) -> Result<(), String> {
        // First delete existing data for this file
        self.delete_file_data(project_id, relative_path).await?;

        // Read the file content
        let content = tokio::fs::read_to_string(full_path)
            .await
            .map_err(|e| format!("Failed to read file: {}", e))?;

        // Determine language from extension
        let ext = full_path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let language = match ext {
            "rs" => "rust",
            "py" => "python",
            "ts" | "tsx" => "typescript",
            "js" | "jsx" => "javascript",
            "go" => "go",
            _ => return Err(format!("Unsupported extension: {}", ext)),
        };

        // Parse the file
        let parse_result = indexer::parse_file(&content, language)
            .map_err(|e| format!("Parse error: {}", e))?;

        let conn = self.db.conn();

        // Insert symbols
        for symbol in &parse_result.symbols {
            conn.execute(
                "INSERT INTO code_symbols (project_id, file_path, name, symbol_type, start_line, end_line, signature)
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
                rusqlite::params![
                    project_id,
                    relative_path,
                    symbol.name,
                    symbol.kind,
                    symbol.start_line,
                    symbol.end_line,
                    symbol.signature
                ],
            ).map_err(|e| e.to_string())?;
        }

        // Insert imports
        for import in &parse_result.imports {
            conn.execute(
                "INSERT OR IGNORE INTO imports (project_id, file_path, import_path, is_external)
                 VALUES (?, ?, ?, ?)",
                rusqlite::params![project_id, relative_path, import.path, import.is_external],
            ).map_err(|e| e.to_string())?;
        }

        // Note: embeddings not created here - run 'index' to generate embeddings

        tracing::debug!(
            "Updated file {} in project {}: {} symbols",
            relative_path, project_id, parse_result.symbols.len()
        );

        Ok(())
    }
}

/// Shared watcher handle for registering projects
#[derive(Clone)]
pub struct WatcherHandle {
    watched_projects: Arc<RwLock<HashMap<i64, PathBuf>>>,
}

impl WatcherHandle {
    /// Register a project for watching
    pub async fn watch(&self, project_id: i64, project_path: PathBuf) {
        let mut projects = self.watched_projects.write().await;
        projects.entry(project_id).or_insert_with(|| {
            tracing::info!("Registering project {} for file watching at {:?}", project_id, project_path);
            project_path
        });
    }

    /// Unregister a project from watching
    pub async fn unwatch(&self, project_id: i64) {
        let mut projects = self.watched_projects.write().await;
        projects.remove(&project_id);
    }
}

/// Spawn the file watcher and return a handle for registering projects
pub fn spawn(
    db: Arc<Database>,
    shutdown: watch::Receiver<bool>,
) -> WatcherHandle {
    let watched_projects = Arc::new(RwLock::new(HashMap::new()));
    let handle = WatcherHandle {
        watched_projects: watched_projects.clone(),
    };

    let watcher = FileWatcher {
        db,
        watched_projects,
        pending_changes: Arc::new(RwLock::new(HashMap::new())),
        shutdown,
    };

    tokio::spawn(async move {
        watcher.run().await;
    });

    handle
}
