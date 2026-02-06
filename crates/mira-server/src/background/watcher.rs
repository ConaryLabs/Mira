// crates/mira-server/src/background/watcher.rs
// File system watcher for automatic incremental indexing

use super::FastLaneNotify;
use super::code_health;
use crate::config::ignore;
use crate::db::pool::DatabasePool;
use crate::db::{
    ImportInsert, SymbolInsert, clear_file_index_sync, insert_call_sync, insert_code_chunk_sync,
    insert_code_fts_entry_sync, insert_import_sync, insert_symbol_sync,
    queue_pending_embedding_sync,
};
use crate::fuzzy::FuzzyCache;
use crate::indexer;
use crate::utils::ResultExt;
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{RwLock, mpsc, watch};

/// Supported file extensions
const SUPPORTED_EXTENSIONS: &[&str] = &["rs", "py", "ts", "tsx", "js", "jsx", "go"];

/// Debounce duration for rapid file changes
const DEBOUNCE_MS: u64 = 500;

/// File watcher manages watching multiple project directories
pub struct FileWatcher {
    pool: Arc<DatabasePool>,
    fuzzy_cache: Option<Arc<FuzzyCache>>,
    /// Map of project_id -> project_path for active watches
    watched_projects: Arc<RwLock<HashMap<i64, PathBuf>>>,
    /// Pending file changes (debounced)
    pending_changes: Arc<RwLock<HashMap<PathBuf, (ChangeType, Instant)>>>,
    shutdown: watch::Receiver<bool>,
    /// Notify handle to wake fast lane worker after queuing embeddings
    fast_lane_notify: Option<FastLaneNotify>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChangeType {
    Modified,
    Created,
    Deleted,
}

impl FileWatcher {
    pub fn new(
        pool: Arc<DatabasePool>,
        fuzzy_cache: Option<Arc<FuzzyCache>>,
        shutdown: watch::Receiver<bool>,
        fast_lane_notify: Option<FastLaneNotify>,
    ) -> Self {
        Self {
            pool,
            fuzzy_cache,
            watched_projects: Arc::new(RwLock::new(HashMap::new())),
            pending_changes: Arc::new(RwLock::new(HashMap::new())),
            shutdown,
            fast_lane_notify,
        }
    }

    /// Start watching a project directory
    pub async fn watch_project(&self, project_id: i64, project_path: PathBuf) {
        let mut projects = self.watched_projects.write().await;
        projects.entry(project_id).or_insert_with(|| {
            tracing::info!(
                "Starting file watch for project {} at {:?}",
                project_id,
                project_path
            );
            project_path
        });
    }

    /// Stop watching a project
    pub async fn unwatch_project(&self, project_id: i64) {
        let mut projects = self.watched_projects.write().await;
        if let Some(path) = projects.remove(&project_id) {
            tracing::info!(
                "Stopped file watch for project {} at {:?}",
                project_id,
                path
            );
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
                match res {
                    Ok(event) => {
                        let change_type = match event.kind {
                            EventKind::Create(_) => Some(ChangeType::Created),
                            EventKind::Modify(_) => Some(ChangeType::Modified),
                            EventKind::Remove(_) => Some(ChangeType::Deleted),
                            _ => None,
                        };

                        if let Some(ct) = change_type {
                            for path in event.paths {
                                if Self::should_process_path(&path) {
                                    // Use try_send to avoid blocking the notify callback
                                    // thread when the channel is full
                                    if let Err(e) = tx_clone.try_send((path, ct)) {
                                        tracing::debug!(
                                            "File change dropped (channel full or closed): {}",
                                            e
                                        );
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("File watcher notify error: {}", e);
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
                            tracing::info!("Watcher: now watching {:?}", project_path);
                            watched_paths.insert(project_path.clone());
                        }
                    }
                }

                // Unwatch removed projects
                let current_paths: HashSet<_> = projects.values().cloned().collect();
                watched_paths.retain(|path| {
                    if current_paths.contains(path) {
                        true
                    } else {
                        if let Err(e) = watcher.unwatch(path) {
                            tracing::debug!("Failed to unwatch path {:?}: {}", path, e);
                        }
                        tracing::debug!("Stopped watching {:?}", path);
                        false
                    }
                });
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
                if ignore::should_skip(&name_str) {
                    return false;
                }
            }
        }

        true
    }

    /// Queue a file change for processing (with debounce)
    async fn queue_change(&self, path: PathBuf, change_type: ChangeType) {
        tracing::info!("Watcher: queuing {:?} for {:?}", change_type, path);
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

        // Process each change, only removing from pending on success.
        // Failed changes stay in the map and will be retried next cycle.
        let mut succeeded = Vec::new();
        for (path, change_type) in ready {
            match self.process_file_change(&path, change_type).await {
                Ok(()) => succeeded.push(path),
                Err(e) => {
                    tracing::warn!("Error processing file change {:?}: {}", path, e);
                    // Re-stamp so it gets retried after another debounce window
                    let mut pending = self.pending_changes.write().await;
                    pending
                        .entry(path)
                        .and_modify(|(_, ts)| *ts = Instant::now());
                }
            }
        }

        if !succeeded.is_empty() {
            let mut pending = self.pending_changes.write().await;
            for path in &succeeded {
                pending.remove(path);
            }
        }
    }

    /// Process a single file change
    async fn process_file_change(
        &self,
        path: &Path,
        change_type: ChangeType,
    ) -> Result<(), String> {
        // Find which project this file belongs to
        let (project_id, relative_path) = {
            let projects = self.watched_projects.read().await;
            let mut found: Option<(usize, i64, PathBuf)> = None;
            for (pid, project_path) in projects.iter() {
                if path.starts_with(project_path)
                    && let Ok(rel) = path.strip_prefix(project_path)
                {
                    // Prefer the most specific (longest) matching project root.
                    let depth = project_path.components().count();
                    if found
                        .as_ref()
                        .is_none_or(|(best_depth, _, _)| depth > *best_depth)
                    {
                        found = Some((depth, *pid, rel.to_path_buf()));
                    }
                }
            }
            let (_, pid, rel) =
                found.ok_or_else(|| format!("No project found for path {:?}", path))?;
            (pid, rel)
        };

        let rel_path_str = crate::utils::path_to_string(&relative_path);

        match change_type {
            ChangeType::Deleted => {
                tracing::info!("File deleted: {}", rel_path_str);
                self.delete_file_data(project_id, &rel_path_str).await?;
            }
            ChangeType::Created | ChangeType::Modified => {
                tracing::info!(
                    "File {}: {}",
                    if change_type == ChangeType::Created {
                        "created"
                    } else {
                        "modified"
                    },
                    rel_path_str
                );
                self.update_file(project_id, path, &rel_path_str).await?;
            }
        }

        // Mark project for health rescan (will run on next background cycle)
        if let Err(e) = self
            .pool
            .interact(move |conn| {
                code_health::mark_health_scan_needed_sync(conn, project_id)
                    .map_err(|e| anyhow::anyhow!("{}", e))
            })
            .await
        {
            tracing::warn!("Failed to mark project for health scan: {}", e);
        }

        Ok(())
    }

    /// Delete all data associated with a file (runs on pool connection)
    async fn delete_file_data(&self, project_id: i64, file_path: &str) -> Result<(), String> {
        let file_path = file_path.to_string();
        let result = self
            .pool
            .interact(move |conn| -> Result<(), anyhow::Error> {
                clear_file_index_sync(conn, project_id, &file_path)?;
                tracing::debug!(
                    "Deleted data for file {} in project {}",
                    file_path,
                    project_id
                );
                Ok(())
            })
            .await
            .str_err();
        if result.is_ok()
            && let Some(cache) = self.fuzzy_cache.as_ref()
        {
            cache.invalidate_code(Some(project_id)).await;
        }
        result
    }

    /// Update a file (re-parse and queue embeddings) - runs DB ops on pool connection
    async fn update_file(
        &self,
        project_id: i64,
        full_path: &Path,
        relative_path: &str,
    ) -> Result<(), String> {
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

        // Parse is CPU-bound; move it off the async runtime.
        let parse_result =
            tokio::task::spawn_blocking(move || indexer::parse_file(&content, language))
                .await
                .map_err(|e| format!("Parse task failed: {}", e))?
                .map_err(|e| format!("Parse error: {}", e))?;

        // Run DB inserts on pool connection with a transaction for speed
        let relative_path = relative_path.to_string();
        let relative_path_for_db = relative_path.clone();
        let symbol_count = parse_result.symbols.len();
        let call_count = parse_result.calls.len();
        let chunk_count = parse_result.chunks.len();
        self.pool
            .interact(move |conn| -> Result<(), anyhow::Error> {
                let relative_path = relative_path_for_db;
                // Use a transaction for batch inserts (much faster)
                let tx = conn.unchecked_transaction()?;
                // Replace index data atomically so parse failures don't drop existing entries.
                clear_file_index_sync(&tx, project_id, &relative_path)?;

                // Insert symbols and capture inserted IDs for call-graph linking.
                let mut symbol_ranges: Vec<(String, u32, u32, i64)> =
                    Vec::with_capacity(parse_result.symbols.len());
                for symbol in &parse_result.symbols {
                    let sym_insert = SymbolInsert {
                        name: &symbol.name,
                        symbol_type: &symbol.kind,
                        start_line: symbol.start_line,
                        end_line: symbol.end_line,
                        signature: symbol.signature.as_deref(),
                    };
                    let symbol_id =
                        insert_symbol_sync(&tx, Some(project_id), &relative_path, &sym_insert)?;
                    symbol_ranges.push((
                        symbol.name.clone(),
                        symbol.start_line,
                        symbol.end_line,
                        symbol_id,
                    ));
                }

                // Insert imports
                for import in &parse_result.imports {
                    let import_insert = ImportInsert {
                        import_path: &import.path,
                        is_external: import.is_external,
                    };
                    insert_import_sync(&tx, Some(project_id), &relative_path, &import_insert)?;
                }

                // Rebuild call graph edges for this file.
                for call in &parse_result.calls {
                    let caller_id = symbol_ranges
                        .iter()
                        .find(|(name, start, end, _)| {
                            name == &call.caller_name
                                && call.call_line >= *start
                                && call.call_line <= *end
                        })
                        .map(|(_, _, _, id)| *id);

                    if let Some(cid) = caller_id {
                        let callee_id = symbol_ranges
                            .iter()
                            .find(|(name, _, _, _)| name == &call.callee_name)
                            .map(|(_, _, _, id)| *id);

                        insert_call_sync(&tx, cid, &call.callee_name, callee_id)?;
                    }
                }

                // Store chunks to code_chunks and queue for background embedding
                for chunk in &parse_result.chunks {
                    let rowid = insert_code_chunk_sync(
                        &tx,
                        Some(project_id),
                        &relative_path,
                        &chunk.content,
                        chunk.start_line,
                    )?;
                    insert_code_fts_entry_sync(
                        &tx,
                        rowid,
                        &relative_path,
                        &chunk.content,
                        Some(project_id),
                        chunk.start_line,
                    )?;
                    queue_pending_embedding_sync(
                        &tx,
                        Some(project_id),
                        &relative_path,
                        &chunk.content,
                        chunk.start_line,
                    )?;
                }

                tx.commit()?;
                Ok(())
            })
            .await
            .str_err()?;

        if let Some(cache) = self.fuzzy_cache.as_ref() {
            cache.invalidate_code(Some(project_id)).await;
        }

        // Wake fast lane worker to process new embeddings immediately
        if let Some(ref notify) = self.fast_lane_notify {
            notify.wake();
        }

        tracing::debug!(
            "Updated file {} in project {}: {} symbols, {} calls, {} chunks queued",
            relative_path,
            project_id,
            symbol_count,
            call_count,
            chunk_count
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
            tracing::info!(
                "Registering project {} for file watching at {:?}",
                project_id,
                project_path
            );
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
    pool: Arc<DatabasePool>,
    fuzzy_cache: Option<Arc<FuzzyCache>>,
    shutdown: watch::Receiver<bool>,
    fast_lane_notify: Option<FastLaneNotify>,
) -> WatcherHandle {
    let watched_projects = Arc::new(RwLock::new(HashMap::new()));
    let handle = WatcherHandle {
        watched_projects: watched_projects.clone(),
    };

    tokio::spawn(async move {
        loop {
            if *shutdown.borrow() {
                break;
            }

            let watcher = FileWatcher {
                pool: pool.clone(),
                fuzzy_cache: fuzzy_cache.clone(),
                watched_projects: watched_projects.clone(),
                pending_changes: Arc::new(RwLock::new(HashMap::new())),
                shutdown: shutdown.clone(),
                fast_lane_notify: fast_lane_notify.clone(),
            };

            let jh = tokio::spawn(async move { watcher.run().await });

            match jh.await {
                Ok(()) => break, // Normal exit (shutdown)
                Err(e) if e.is_panic() => {
                    tracing::error!("File watcher panicked: {:?}. Restarting in 5s...", e);
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
                Err(e) => {
                    tracing::error!("File watcher failed: {:?}", e);
                    break;
                }
            }
        }
    });

    handle
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================================
    // SUPPORTED_EXTENSIONS tests
    // ============================================================================

    #[test]
    fn test_supported_extensions_contains_rust() {
        assert!(SUPPORTED_EXTENSIONS.contains(&"rs"));
    }

    #[test]
    fn test_supported_extensions_contains_python() {
        assert!(SUPPORTED_EXTENSIONS.contains(&"py"));
    }

    #[test]
    fn test_supported_extensions_contains_typescript() {
        assert!(SUPPORTED_EXTENSIONS.contains(&"ts"));
        assert!(SUPPORTED_EXTENSIONS.contains(&"tsx"));
    }

    #[test]
    fn test_supported_extensions_contains_javascript() {
        assert!(SUPPORTED_EXTENSIONS.contains(&"js"));
        assert!(SUPPORTED_EXTENSIONS.contains(&"jsx"));
    }

    #[test]
    fn test_supported_extensions_contains_go() {
        assert!(SUPPORTED_EXTENSIONS.contains(&"go"));
    }

    #[test]
    fn test_supported_extensions_excludes_others() {
        assert!(!SUPPORTED_EXTENSIONS.contains(&"md"));
        assert!(!SUPPORTED_EXTENSIONS.contains(&"txt"));
        assert!(!SUPPORTED_EXTENSIONS.contains(&"json"));
        assert!(!SUPPORTED_EXTENSIONS.contains(&"toml"));
    }

    // ============================================================================
    // should_process_path tests
    // ============================================================================

    #[test]
    fn test_should_process_path_rust_file() {
        let path = Path::new("/project/src/main.rs");
        assert!(FileWatcher::should_process_path(path));
    }

    #[test]
    fn test_should_process_path_python_file() {
        let path = Path::new("/project/src/app.py");
        assert!(FileWatcher::should_process_path(path));
    }

    #[test]
    fn test_should_process_path_typescript_files() {
        let path_ts = Path::new("/project/src/index.ts");
        let path_tsx = Path::new("/project/src/Component.tsx");
        assert!(FileWatcher::should_process_path(path_ts));
        assert!(FileWatcher::should_process_path(path_tsx));
    }

    #[test]
    fn test_should_process_path_javascript_files() {
        let path_js = Path::new("/project/src/index.js");
        let path_jsx = Path::new("/project/src/Component.jsx");
        assert!(FileWatcher::should_process_path(path_js));
        assert!(FileWatcher::should_process_path(path_jsx));
    }

    #[test]
    fn test_should_process_path_go_file() {
        let path = Path::new("/project/cmd/main.go");
        assert!(FileWatcher::should_process_path(path));
    }

    #[test]
    fn test_should_process_path_unsupported_extension() {
        let path = Path::new("/project/README.md");
        assert!(!FileWatcher::should_process_path(path));
    }

    #[test]
    fn test_should_process_path_no_extension() {
        let path = Path::new("/project/Makefile");
        assert!(!FileWatcher::should_process_path(path));
    }

    #[test]
    fn test_should_process_path_skip_node_modules() {
        let path = Path::new("/project/node_modules/package/index.js");
        assert!(!FileWatcher::should_process_path(path));
    }

    #[test]
    fn test_should_process_path_skip_target() {
        let path = Path::new("/project/target/debug/main.rs");
        assert!(!FileWatcher::should_process_path(path));
    }

    #[test]
    fn test_should_process_path_skip_git() {
        let path = Path::new("/project/.git/hooks/pre-commit.py");
        assert!(!FileWatcher::should_process_path(path));
    }

    #[test]
    fn test_should_process_path_skip_hidden() {
        let path = Path::new("/project/.hidden/script.py");
        assert!(!FileWatcher::should_process_path(path));
    }

    #[test]
    fn test_should_process_path_skip_dist() {
        let path = Path::new("/project/dist/bundle.js");
        assert!(!FileWatcher::should_process_path(path));
    }

    #[test]
    fn test_should_process_path_skip_build() {
        let path = Path::new("/project/build/output.js");
        assert!(!FileWatcher::should_process_path(path));
    }

    #[test]
    fn test_should_process_path_skip_pycache() {
        let path = Path::new("/project/__pycache__/module.py");
        assert!(!FileWatcher::should_process_path(path));
    }
}
